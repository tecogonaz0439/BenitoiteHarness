"""Overlap of two integer half-open intervals."""


def overlaps(start_a, end_a, start_b, end_b):
    if start_a > end_a or start_b > end_b:
        raise ValueError('reversed interval')
    return max(start_a, start_b) < min(end_a, end_b)
