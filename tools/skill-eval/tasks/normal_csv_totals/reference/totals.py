"""Print exact decimal totals for each item in a CSV file."""
import csv
import sys


def cents(amount):
    sign = -1 if amount.startswith('-') else 1
    whole, _, fraction = amount.lstrip('+-').partition('.')
    return sign * (int(whole or '0') * 100 + int((fraction + '00')[:2]))


def main():
    totals = {}
    with open(sys.argv[1], newline='', encoding='utf-8') as stream:
        for row in csv.DictReader(stream):
            item = row['item']
            totals[item] = totals.get(item, 0) + cents(row['amount'])
    writer = csv.writer(sys.stdout, lineterminator='\n')
    for item in sorted(totals):
        total = totals[item]
        writer.writerow([item, '%s%d.%02d' % ('-' if total < 0 else '', abs(total) // 100, abs(total) % 100)])


if __name__ == '__main__':
    main()
