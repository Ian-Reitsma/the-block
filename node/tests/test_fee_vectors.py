import csv
from the_block import fee_decompose


def test_fee_vectors():
    with open("tests/vectors/fee_v2_vectors.csv") as f:
        reader = csv.DictReader(f)
        rows = list(reader)
    for row in rows:
        selector = int(row["selector"])
        fee = int(row["fee"])
        fee_consumer = int(row["fee_consumer"])
        fee_industrial = int(row["fee_industrial"])
        assert fee_decompose(selector, fee) == (fee_consumer, fee_industrial)
