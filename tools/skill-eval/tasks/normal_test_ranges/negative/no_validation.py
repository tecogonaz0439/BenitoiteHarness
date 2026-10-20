"""Overlap of two integer half-open intervals."""


def overlaps(start_a, end_a, start_b, end_b):
    return max(start_a, start_b) < min(end_a, end_b)
