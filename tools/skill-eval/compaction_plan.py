#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""圧縮の課題の依頼の区切りと資料の量を、LLM を呼ばずに見積もる（実装プラン P5-36）。

設計書 07-02「M の段の圧縮」の 3 条件について、各依頼の入口と途中の見積もりを計算し、
最初の圧縮が依頼の入口で起きるか、予備の比較で圧縮の時点が違うかを確かめる。
固定の部分（システムプロンプト、道具、依頼、資料）は実際のバイト数を数え、モデルの出力と
資料以外の読み取りの量は、2026-10-21 の試しの走行で観察した範囲（P5-36 の「調査の結果」）を置く。
この範囲は予測ではなく、観察した量でも条件が崩れないかを確かめるための検査の値である。
"""
import argparse
import itertools
import json
import os
from typing import Dict, Iterable, List, Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
EXPECTED = os.path.join(ROOT, 'crates', 'benitoiteh', 'tests', 'expected')
TASKS = ('compact_json_summary', 'compact_dir_walk')

# 条件ごとの (圧縮の境, 実効の残す量)。context_window 65,536、max_output_tokens 8,192 のとき（設計書 07-02）。
CONDITIONS = {'base': (40960, 16384), 'reserve': (49152, 16384), 'keep': (40960, 8000)}
# 2026-10-21 の記録の最初の依頼にあった環境の部分。記録の 1,312 B から共通の依頼文の 595 B を除いた。
ENVIRONMENT_BYTES = 717
# 観察した範囲（全バイト数を 3 で割ったトークン）。最初の実装の依頼は、言語の資料の読み取りを含む
# 段 1 の資料以外の増分（14,8xx と 22,5xx）、後の実装の依頼は段 2 の資料以外の増分（4,1xx と 5,7xx）を基にした。
FIRST_IMPLEMENTATION = (15000, 22500)
LATER_IMPLEMENTATION = (4000, 6000)
# 調査の依頼の返答と推論。試しの走行の段の最後の応答（文と推論で 524〜1,845 B）より多めに置く。
STUDY_REPLY = (1000, 3000)
# 提供者の入力の使用量と、全バイト数を 3 で割った値の比。段末で 0.79〜0.87 だった。
USAGE_RATIO = (0.79, 0.87)
# 実装の依頼の最後の道具の結果の上限。途中の見積もりでは、この分だけ全バイト数で数える。
LAST_RESULT = 3000
# 圧縮の後の要約と、実際に残る量の残す量に対する比。推論を除いて送るので、kept_estimate より少ない。
SUMMARY = (1500, 3000)
KEPT_RATIO = (0.5, 1.0)


def numbered_bytes(data: bytes) -> int:
    """read の結果の量。各行を六桁の右寄せの番号、タブ、本文、改行として数える。"""
    lines = data.decode('utf-8').split('\n')
    if lines and lines[-1] == '':
        lines.pop()
    return sum(7 + len(line.encode('utf-8')) + 1 for line in lines)


def fixed_bytes() -> int:
    with open(os.path.join(EXPECTED, 'system-restricted.txt'), 'rb') as f:
        system = len(f.read())
    with open(os.path.join(EXPECTED, 'tools-restricted.json'), encoding='utf-8') as f:
        tools = json.load(f)
    return system + len(json.dumps(tools, ensure_ascii=False, separators=(',', ':')).encode('utf-8'))


def common_prompt_bytes() -> int:
    import skill_eval  # 評価の道具と同じ共通の依頼文を数える。
    return len((skill_eval.PROMPT.format(limit=20) + '\n\n').encode('utf-8'))


def task_shape(task: str) -> Tuple[List[int], List[int]]:
    """依頼ごとの (依頼のバイト数, 資料を全部読んだ結果のバイト数)。"""
    root = os.path.join(HERE, 'tasks', task)
    with open(os.path.join(root, 'task.json'), encoding='utf-8') as f:
        config = json.load(f)
    requests, materials = [], []
    for number, stage in enumerate(config['stages'], 1):
        with open(os.path.join(root, stage), 'rb') as f:
            requests.append(len(f.read()))
        total = 0
        directory = os.path.join(root, 'materials', str(number))
        for parent, _dirs, files in os.walk(directory):
            for name in files:
                with open(os.path.join(parent, name), 'rb') as f:
                    total += numbered_bytes(f.read())
        materials.append(total)
    return requests, materials


def tokens(value: float) -> int:
    return int(-(-value // 3))


def simulate(fixed: int, requests: List[int], materials: List[int], replies: List[int], ratio: float,
             threshold: int, keep: int, summary: int, kept_ratio: float, read: bool = True) -> List[Dict[str, object]]:
    """圧縮の出来事を (依頼, 'entry' か 'mid', 前の見積もり) で返す。量はトークンで扱う。"""
    events = []
    size = tokens(fixed) + 0.0
    for index, (request, material, reply) in enumerate(zip(requests, materials, replies)):
        number = index + 1
        size += request / 3
        # 再開の入口は全バイト数で見積もる（P5-36 の「段の境の増分」）。
        if size > threshold:
            events.append(dict(request=number, where='entry', before=round(size)))
            size = tokens(fixed) + request / 3 + summary + kept_ratio * min(keep, size - tokens(fixed))
        material_tokens = material / 3 if read else 0
        if material:
            # 調査の依頼: 一度に読み、その結果を加えた後の番の前に判定する。返答の後には番がない。
            middle = ratio * size + material_tokens + reply
            end = size + material_tokens + reply
        else:
            end = size + reply
            middle = ratio * end + (1 - ratio) * LAST_RESULT
        if middle > threshold:
            events.append(dict(request=number, where='mid', before=round(middle)))
            # 道具の呼び出しが続く途中の圧縮は、Anthropic の形の口では残す範囲を設けない（01-04「圧縮」）。
            end = tokens(fixed) + summary + min(reply, LAST_RESULT)
        size = end
    return events


def scenarios(materials: List[int]) -> Iterable[Tuple[List[int], float, int, float]]:
    """観察した範囲の端と中を組み合わせる。調査の返答と後の実装の量は依頼ごとに別に動かす。"""
    def points(low: int, high: int, count: int) -> List[int]:
        return [low + (high - low) * step // (count - 1) for step in range(count)]
    slots = []
    first_implementation = True
    for material in materials:
        if material:
            slots.append(points(*STUDY_REPLY, 3))
        elif first_implementation:
            slots.append(points(*FIRST_IMPLEMENTATION, 4))
            first_implementation = False
        else:
            slots.append(points(*LATER_IMPLEMENTATION, 3))
    for replies in itertools.product(*slots):
        for ratio in USAGE_RATIO:
            for summary in SUMMARY:
                for kept_ratio in KEPT_RATIO:
                    yield list(replies), ratio, summary, kept_ratio


def check(task: str, fixed: Optional[int] = None) -> Dict[str, object]:
    """全部読む場合に、全条件の最初の圧縮が入口で起き、予備の条件が後の入口で圧縮するかを数える。"""
    fixed = fixed_bytes() + ENVIRONMENT_BYTES + common_prompt_bytes() if fixed is None else fixed
    requests, materials = task_shape(task)
    failures, total = [], 0
    first = {name: set() for name in CONDITIONS}
    later_mid = {name: 0 for name in CONDITIONS}
    for replies, ratio, summary, kept_ratio in scenarios(materials):
        total += 1
        found = {}
        for name, (threshold, keep) in CONDITIONS.items():
            events = simulate(fixed, requests, materials, replies, ratio, threshold, keep, summary, kept_ratio)
            found[name] = events
            if events:
                first[name].add('%d-%s' % (events[0]['request'], events[0]['where']))
            later_mid[name] += any(event['where'] == 'mid' for event in events[1:])
        problems = [name + ': no compaction' for name, events in found.items() if not events]
        problems += [name + ': first compaction inside request %d' % events[0]['request']
                     for name, events in found.items() if events and events[0]['where'] != 'entry']
        if found['base'] and found['reserve'] and found['reserve'][0]['request'] <= found['base'][0]['request']:
            problems.append('reserve does not compact at a later request')
        if problems:
            failures.append(dict(replies=replies, ratio=ratio, summary=summary, kept_ratio=kept_ratio,
                                 problems=problems))
    unread = {name: simulate(fixed, requests, materials, [STUDY_REPLY[1] if m else LATER_IMPLEMENTATION[1]
                                                            for m in materials], USAGE_RATIO[1], threshold, keep,
                             SUMMARY[1], KEPT_RATIO[1], read=False)
              for name, (threshold, keep) in CONDITIONS.items()}
    return dict(task=task, fixed_tokens=tokens(fixed), requests=requests, materials=materials,
                scenarios=total, failures=failures, first={k: sorted(v) for k, v in first.items()},
                later_mid=later_mid, unread_compactions={k: len(v) for k, v in unread.items()})


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--task', choices=TASKS, action='append')
    parser.add_argument('--json', action='store_true', help='print the full result as JSON')
    args = parser.parse_args(argv)
    status = 0
    for task in args.task or TASKS:
        result = check(task)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            print('%s: fixed %d tokens, materials %s B, %d scenarios, %d failing; first %s; later mid %s; '
                  'unread compactions %s' % (task, result['fixed_tokens'], result['materials'], result['scenarios'],
                                             len(result['failures']), result['first'], result['later_mid'],
                                             result['unread_compactions']))
            for failure in result['failures'][:5]:
                print('  ', failure)
        status |= bool(result['failures'])
    return status


if __name__ == '__main__':
    raise SystemExit(main())
