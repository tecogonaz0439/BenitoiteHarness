"""Remove repeated lines, retaining the first spelling and order."""
import argparse
import sys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--ignore-case', action='store_true')
    parser.add_argument('input')
    args = parser.parse_args()
    seen = set()
    with open(args.input, encoding='utf-8', newline='') as stream:
        for raw in stream:
            line = raw.rstrip('\r\n')
            key = line.casefold() if args.ignore_case else line
            if key not in seen:
                seen.add(key)
                sys.stdout.write(line + '\n')


if __name__ == '__main__':
    main()
