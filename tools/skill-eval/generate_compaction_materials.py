#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""段ごとの調査例を決定的に生成する（実装プラン P5-33）。"""
import argparse
import hashlib
import json
from pathlib import Path

TASKS = ('compact_json_summary', 'compact_dir_walk')
HERE = Path(__file__).resolve().parent


def encoded(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(',', ':'))


def example(task, stage, number, seed):
    """各例の値と期待する中間結果を、乱数や解の実行を使わずに作る。"""
    n = number + seed
    name = 'sample-%06d' % number
    if task == 'compact_json_summary':
        amount = 11 + n % 997
        if stage == 1:
            orders = [dict(id=name + '-a', region='west', amount=amount, status='paid'),
                      dict(id=name + '-b', region='east', status='paid'),
                      dict(id=name + '-c', region='west', amount=amount + 7, status='cancelled'),
                      dict(id=name + '-d', region='east', amount=amount + 3, status='paid')]
            result = dict(east=dict(count=2, amount=amount + 3), west=dict(count=1, amount=amount))
            return '## %s\nOrders: %s\nRegional subtotals: %s\nOmitted order ID: %s-c.\n\n' % (
                name, encoded(orders), encoded(result), name)
        if stage == 2:
            unknown = 'market-%04d' % n
            orders = [dict(id=name + '-a', region=unknown, amount=amount, status='paid'),
                      dict(id=name + '-b', region='east', amount=2, status='paid'),
                      dict(id=name + '-c', region='remote-%04d' % n, amount=5, status='paid')]
            result = dict(east=dict(count=1, amount=2), other=dict(count=2, amount=amount + 5))
            return '## %s\nNew market batch: %s\nreport.json value: %s\n\n' % (name, encoded(orders), encoded(result))
        return ('## %s\nLaunch input: batches/%s.json\nInput array: %s\n'
                'Selected source: batches/%s.json; independent default orders.json: %s\n'
                'Selected-source subtotal: %s\n\n') % (
                    name, name, encoded([dict(region='east', amount=amount, status='paid')]), name,
                    encoded([dict(region='west', amount=amount + 1, status='paid')]),
                    encoded(dict(east=dict(count=1, amount=amount))))
    section = 'chapter-%04d' % n
    if stage == 1:
        paths = [section + '/intro.md', section + '/drafts/notes.md', section + '/.cache/index.md',
                 section + '/guide/deep/topic.md', section + '/todo.txt', section + '/.visible.md']
        selected = sorted([paths[0], paths[3], paths[5]])
        return ('## %s\nObserved relative files: %s\nSelected relative files: %s\n'
                'Excluded subtrees: %s; unrelated text file: %s\n\n') % (
                    name, encoded(paths), encoded(selected), encoded([section + '/drafts', section + '/.cache']), paths[4])
    if stage == 2:
        paths = [section + '/intro.MD', section + '/guide/Topic.mD', section + '/guide/extra.md',
                 section + '/assets/logo.png']
        return '## %s\nObserved files: %s\ncounts.json value: %s\n\n' % (
            name, encoded(paths), encoded({section: 1, section + '/guide': 2}))
    return ('## %s\nLaunch root: snapshots/%s\nRelative files below that root: %s\n'
            'Printed relative paths: %s\nIndependent default docs/ entry: unrelated-%04d.md\n\n') % (
                name, name, encoded([section + '/readme.md', 'root.md']),
                encoded([section + '/readme.md', 'root.md']), n)


def generate(task, total_kib, chunk_kib, seed):
    """目標量以上となる最小の完全な例まで書き、一つの資料を上限内に収める。"""
    files = {}
    serial = 0
    for stage in range(1, 4):
        budget = total_kib * 1024 // 3 + (1 if stage <= total_kib * 1024 % 3 else 0)
        written = 0
        part = 0
        while written < budget:
            part += 1
            body = '# Stage %d field notes, part %02d\n\n' % (stage, part)
            target = min(chunk_kib * 1024, budget - written)
            while len(body.encode('utf-8')) < target:
                record = example(task, stage, serial, seed)
                if len((body + record).encode('utf-8')) > 12 * 1024:
                    break
                body += record
                serial += 1
            data = body.encode('utf-8')
            files['materials/%d/notes-%02d.md' % (stage, part)] = data
            written += len(data)
    return files


def manifest(task, total_kib, chunk_kib, seed, files):
    hashes = {path: hashlib.sha256(data).hexdigest() for path, data in sorted(files.items())}
    # パスも集約に含め、内容が同じファイルの入れ替えを識別する。
    aggregate = ''.join('%s  %s\n' % (digest, path) for path, digest in sorted(hashes.items()))
    return dict(generator='generate_compaction_materials.py', version=1,
                arguments=dict(total_kib=total_kib, chunk_kib=chunk_kib, seed=seed),
                initial_candidates_kib=[24, 48, 64], initial_candidate_kib=64,
                added_units_kib=[12] * ((total_kib - 64) // 12) if total_kib >= 64 else [],
                length_status='provisional; fix common length and stages after paid pilot',
                bytes=sum(map(len, files.values())),
                stage_bytes={str(stage): sum(len(data) for path, data in files.items()
                                            if path.startswith('materials/%d/' % stage)) for stage in range(1, 4)},
                sha256=hashlib.sha256(aggregate.encode('ascii')).hexdigest(), files_sha256=hashes)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--task', choices=TASKS, action='append')
    parser.add_argument('--total-kib', type=int, default=268)
    parser.add_argument('--chunk-kib', type=int, default=10)
    parser.add_argument('--seed', type=int, default=33)
    parser.add_argument('--tasks-dir', type=Path, default=HERE / 'tasks')
    parser.add_argument('--check', action='store_true', help='compare bytes and manifest without writing')
    args = parser.parse_args(argv)
    if args.total_kib <= 0 or not 1 <= args.chunk_kib <= 12:
        parser.error('total-kib must be positive; chunk-kib must be between 1 and 12')
    for task in args.task or TASKS:
        root = args.tasks_dir / task
        files = generate(task, args.total_kib, args.chunk_kib, args.seed)
        generation = manifest(task, args.total_kib, args.chunk_kib, args.seed, files)
        config = json.loads((root / 'task.json').read_text(encoding='utf-8'))
        if args.check:
            actual = {p.relative_to(root).as_posix(): p.read_bytes() for p in (root / 'materials').rglob('*') if p.is_file()}
            if actual != files or config.get('generation') != generation:
                parser.exit(1, '%s: generated materials or generation manifest differ\n' % task)
        else:
            # 不意に入力や解を消さないよう、生成する資料だけを置き換える。
            for path in sorted((root / 'materials').rglob('*'), reverse=True):
                if path.is_file():
                    path.unlink()
                elif path.is_dir():
                    path.rmdir()
            for path, data in files.items():
                destination = root / path
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(data)
            config['generation'] = generation
            (root / 'task.json').write_text(json.dumps(config, ensure_ascii=True, indent=2, sort_keys=True) + '\n', encoding='utf-8')
        print('%s: %d bytes, %d files, sha256=%s' % (task, generation['bytes'], len(files), generation['sha256']))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
