#!/usr/bin/env python3
"""macOS の手動測定を集める（設計書 06-02「測り方」、実装プラン P5-05）。"""

import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
import tempfile

# 目標のメモリを、測定値と同じ KiB で持つ。
ITEMS = {
    'version_ms': ('ms', 5),
    'exec_first_request_ms': ('ms', 30),
    'exec_first_request_restricted_ms': ('ms', None),
    'tui_ready_ms': ('ms', 150),
    'tui_idle_rss_kib': ('KiB', 20480),
    'session_200_max_rss_kib': ('KiB', 102400),
    'stream_delta_ms': ('ms', 16),
    'grep_10k_first_ms': ('ms', 1000),
    'grep_10k_ms': ('ms', 1000),
}


def parse_perf(output):
    """libtest の行頭の表示を許し、壊れた行でもほかの標本を残す。"""
    records, errors = {}, []
    for number, line in enumerate(output.splitlines(), 1):
        start = line.find('PERF ')
        if start < 0:
            continue
        try:
            record = json.loads(line[start + len('PERF '):])
            if not isinstance(record, dict):
                raise ValueError('expected an object')
            item = record.get('item')
            if not isinstance(item, str) or item not in ITEMS:
                raise ValueError('unknown item')
            unit = ITEMS[item][0]
            if record.get('unit') != unit or not isinstance(record.get('note'), str):
                raise ValueError('invalid unit or note')
            samples = record.get('samples')
            if not isinstance(samples, list) or not samples:
                raise ValueError('expected nonempty samples')
            for sample in samples:
                if (type(sample) not in (int, float) or not math.isfinite(sample)
                        or sample < 0 or (unit == 'KiB' and type(sample) is not int)):
                    raise ValueError('samples must be finite nonnegative numbers (integer KiB)')
            if item in records:
                raise ValueError('duplicate item: ' + item)
            records[item] = record
        except (ValueError, TypeError, OverflowError) as error:
            errors.append('PERF line {}: {}'.format(number, error))
    return records, errors


def missing_items(records):
    return [item for item in ITEMS if item not in records]


def summary(samples, target):
    median = statistics.median(samples)
    maximum = max(samples)
    if target is None:
        verdict = '参考（判定なし）'
    else:
        verdict = '達成' if median <= target else '未達'
        if maximum > target:
            verdict += '（最大値は目標超過）'
    return len(samples), median, maximum, verdict


def cell(value):
    return str(value).replace('|', '\\|').replace('\r', '').replace('\n', '<br>')


def table(configurations):
    """各構成の中央値を判定し、欠測を空欄にしない比較表を作る。"""
    header = ['項目', '単位', '目標']
    for config in configurations:
        header.extend(config['name'] + ' ' + label for label in ('回数', '中央値', '最大値', '判定'))
    lines = ['| ' + ' | '.join(header) + ' |', '| ' + ' | '.join(['---'] * len(header)) + ' |']
    rows = [('binary_size_mib', 'MiB', 8)] + [(item, unit, target) for item, (unit, target) in ITEMS.items()]
    for item, unit, target in rows:
        row = [item, unit, '参考' if target is None else str(target)]
        for config in configurations:
            record = config['records'].get(item)
            if record is None:
                row.extend(['0', '測れなかった', '測れなかった', '測れなかった'])
            else:
                count, median, maximum, verdict = summary(record['samples'], target)
                row.extend([str(count), '{:.3f}'.format(median), '{:.3f}'.format(maximum), verdict])
        lines.append('| ' + ' | '.join(row) + ' |')
    return '\n'.join(lines)


def report(environment, configurations):
    lines = ['# benitoiteh 性能の測定', '', '測定値はこの実行環境の値である。判定は中央値で行う。', '',
             '## 環境', '', '| 項目 | 値 |', '|---|---|']
    lines.extend('| {} | {} |'.format(cell(key), cell(value)) for key, value in environment.items())
    lines.extend(['', '## 測定値', '', table(configurations), '', '## 条件と失敗', ''])
    for config in configurations:
        lines.append('### ' + config['name'])
        lines.append('')
        for item, record in config['records'].items():
            lines.append('- `{}`: {}'.format(item, cell(record['note'])))
        for error in config['errors']:
            lines.append('- 測定失敗: ' + cell(error))
        lines.extend(['', '標本（PERF の JSON）:', '', '```jsonl'])
        lines.extend(json.dumps(record, ensure_ascii=False, allow_nan=False) for record in config['records'].values())
        lines.extend(['```', ''])
    return '\n'.join(lines)


def cargo_command(action, opt_s=False):
    args = ['cargo', action, '--offline', '--locked', '--release', '-p', 'benitoiteh']
    if opt_s:
        args.extend(['--config', 'profile.release.opt-level="s"', '--target-dir', 'target/perf-opt-s'])
    if action == 'test':
        args.extend(['--test', 'perf_measure', '--', '--ignored', '--nocapture', '--test-threads=1'])
    return args


def sha256(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def execute(args, root, env):
    # シェルを通さず、パスの空白も一つの引数として保つ。
    return subprocess.run(args, cwd=str(root), env=env, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, text=True, check=False)


def collect_environment(root, processor, env):
    values = {'UTC': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    commands = [('Rust', ['rustc', '-V']), ('commit', ['git', 'rev-parse', 'HEAD']),
                ('macOS', ['sw_vers']), ('CPU', ['sysctl', '-n', 'machdep.cpu.brand_string']),
                ('benitoite version', [str(processor), '--version=json'])]
    for name, args in commands:
        try:
            result = execute(args, root, env)
            values[name] = result.stdout.strip() if result.returncode == 0 else '取得失敗: ' + result.stdout.strip()
        except OSError as error:
            values[name] = '取得失敗: ' + str(error)
    status = execute(['git', 'status', '--porcelain'], root, env)
    values['working tree clean'] = str(status.returncode == 0 and not status.stdout.strip())
    values['Cargo.lock SHA-256'] = sha256(root / 'Cargo.lock')
    values['benitoite path'] = str(processor)
    values['benitoite bytes'] = str(processor.stat().st_size)
    values['benitoite SHA-256'] = sha256(processor)
    manifest = (root / 'Cargo.toml').read_text(encoding='utf-8')
    profile = re.search(r'^\[profile\.release\]\s*\n([^\[]*)', manifest, re.MULTILINE)
    values['[profile.release]'] = profile.group(1).strip() if profile else '見つからなかった'
    values['features'] = 'Cargo default features; crates/benitoiteh/Cargo.toml at the recorded commit and working tree'
    values['cargo target'] = 'host; CARGO_TARGET_DIR and CARGO_BUILD_TARGET removed'
    for name, value in sorted(env.items()):
        if name.startswith('CARGO_PROFILE_RELEASE_') or name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS'):
            values[name] = value
    return values


def measure_configuration(root, processor, count, env, opt_s):
    name = 'opt-s' if opt_s else 'default'
    config = {'name': name, 'records': {}, 'errors': []}
    test_env = dict(env, BENITOITEH_TEST_BENITOITE=str(processor), BENITOITEH_PERF_RUNS=str(count))
    try:
        build_args = cargo_command('build', opt_s)
        build = execute(build_args, root, test_env)
        print(build.stdout, file=sys.stderr, end='')
        if build.returncode != 0:
            raise RuntimeError('cargo build exited {}'.format(build.returncode))
        target = root / ('target/perf-opt-s' if opt_s else 'target')
        snapshot = root / 'target/perf' / ('benitoiteh-' + name)
        snapshot.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(target / 'release/benitoiteh', snapshot)
        size = snapshot.stat().st_size
        digest = sha256(snapshot)
        config['records']['binary_size_mib'] = {
            'item': 'binary_size_mib', 'unit': 'MiB', 'samples': [size / (1024 * 1024)],
            'note': 'bytes={}; SHA-256={}; binary={}; --config={}'.format(
                size, digest, snapshot, 'profile.release.opt-level="s"' if opt_s else '(none)')}
        test_env['BENITOITEH_PERF_BINARY'] = str(snapshot)
        result = execute(cargo_command('test', opt_s), root, test_env)
        print(result.stdout, file=sys.stderr, end='')
        records, errors = parse_perf(result.stdout)
        config['records'].update(records)
        config['errors'].extend(errors)
        if result.returncode != 0:
            config['errors'].append('cargo test exited {}'.format(result.returncode))
    except (OSError, RuntimeError) as error:
        config['errors'].append(str(error))
    for item in missing_items(config['records']):
        config['errors'].append(item + ': 測れなかった')
    return config


def positive(value):
    try:
        count = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError('runs must be a positive integer') from error
    if count < 1:
        raise argparse.ArgumentTypeError('runs must be at least 1')
    return count


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--benitoite', required=True, type=Path)
    parser.add_argument('--runs', type=positive, default=10)
    parser.add_argument('--out', type=Path)
    parser.add_argument('--opt-s', action='store_true')
    args = parser.parse_args(argv)
    if sys.platform != 'darwin':
        parser.error('performance measurements require macOS')
    root = Path(__file__).resolve().parents[2]
    processor = args.benitoite.resolve(strict=True)
    # 親の設定は子へ渡さず、処理系の版の問い合わせも隔離する。
    with tempfile.TemporaryDirectory(prefix='benitoiteh-perf-driver-') as temporary:
        env = os.environ.copy()
        for variable in ('HOME', 'XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'TMPDIR'):
            location = Path(temporary) / variable.lower()
            location.mkdir()
            env[variable] = str(location)
        # HOME を移しても、既存の Rust ツールチェーンとオフラインのキャッシュを使う。
        for variable, directory in (('CARGO_HOME', '.cargo'), ('RUSTUP_HOME', '.rustup')):
            env[variable] = os.environ.get(variable, str(Path.home() / directory))
        for variable in ('CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET', 'BENITOITEH_PERF_BINARY'):
            env.pop(variable, None)
        env['CARGO_NET_OFFLINE'] = 'true'
        environment = collect_environment(root, processor, env)
        environment['runs'] = str(args.runs)
        environment['--config override'] = 'profile.release.opt-level="s" (opt-s only)' if args.opt_s else '(none)'
        configurations = [measure_configuration(root, processor, args.runs, env, False)]
        if args.opt_s:
            configurations.append(measure_configuration(root, processor, args.runs, env, True))
        markdown = report(environment, configurations)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(markdown + '\n', encoding='utf-8')
    else:
        print(markdown)
    return 1 if any(config['errors'] for config in configurations) else 0


if __name__ == '__main__':
    sys.exit(main())
