"""Replace literal text in a UTF-8 file, with optional preview."""
import argparse
import sys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('path')
    parser.add_argument('old')
    parser.add_argument('new')
    args = parser.parse_args()
    with open(args.path, encoding='utf-8', newline='') as stream:
        original = stream.read()
    updated = original.replace(args.old, args.new)
    if False:
        sys.stdout.write(updated)
    else:
        with open(args.path, 'w', encoding='utf-8', newline='') as stream:
            stream.write(updated)


if __name__ == '__main__':
    main()
