"""Sort JSON records while preserving the input order of ties."""
import argparse
import json
import sys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--descending', action='store_true')
    parser.add_argument('input')
    parser.add_argument('key')
    args = parser.parse_args()
    with open(args.input, encoding='utf-8') as stream:
        rows = json.load(stream)
    if any(args.key not in row for row in rows):
        print('missing key: ' + args.key, file=sys.stderr)
        return 2
    ordered = sorted(rows, key=lambda row: row[args.key], reverse=False)
    if args.descending:
        ordered.reverse()
    print(json.dumps(ordered, ensure_ascii=False))
    return 0


if __name__ == '__main__':
    sys.exit(main())
