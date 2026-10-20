"""Print the totals for each item in a CSV file."""
import csv
import sys


def main():
    totals = {}
    with open(sys.argv[1], newline='', encoding='utf-8') as stream:
        for row in csv.DictReader(stream):
            item = row['item']
            totals[item] = totals.get(item, 0.0) + float(row['amount'])
    writer = csv.writer(sys.stdout, lineterminator='\n')
    for item in sorted(totals):
        writer.writerow([item, '%.2f' % (int(totals[item] * 100) / 100)])


if __name__ == '__main__':
    main()
