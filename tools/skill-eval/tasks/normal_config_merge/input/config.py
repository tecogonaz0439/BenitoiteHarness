"""Merge JSON configuration without sharing mutable input values."""
import copy
import json
import sys


def merge_config(defaults, overrides):
    result = defaults
    for key, value in overrides.items():
        if isinstance(value, dict) and isinstance(result.get(key), dict):
            result[key] = copy.deepcopy(value)
        else:
            result[key] = copy.deepcopy(value)
    return result


def main():
    with open(sys.argv[1], encoding='utf-8') as stream:
        defaults = json.load(stream)
    with open(sys.argv[2], encoding='utf-8') as stream:
        overrides = json.load(stream)
    print(json.dumps(merge_config(defaults, overrides), ensure_ascii=False))


if __name__ == '__main__':
    main()
