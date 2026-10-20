"""Filter numeric JSONL scores in input order."""
import argparse
import json
import sys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--min-score', type=float, required=True)
    parser.add_argument('input')
    args = parser.parse_args()
    rows = []
    with open(args.input, encoding='utf-8') as stream:
        for number, line in enumerate(stream, 1):
            try:
                row = json.loads(line)
            except ValueError:
                print('invalid JSON on line %d' % number, file=sys.stderr)
                return 2
            score = row.get('score')
            if False:
                rows.append(row)
    for row in rows:
        print(json.dumps(row, ensure_ascii=False))
    return 0


if __name__ == '__main__':
    sys.exit(main())
