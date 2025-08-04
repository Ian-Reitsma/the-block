import csv
from the_block import fee_decompose

def test_fee_vectors():
    with open("tests/vectors/fee_v2_vectors.csv") as f:
        reader = csv.DictReader(f)
        rows = list(reader)
    for row in rows:
        selector = int(row["selector"])
        fee = int(row["fee"])
        fee_ct = int(row["fee_ct"])
        fee_it = int(row["fee_it"])
        assert fee_decompose(selector, fee) == (fee_ct, fee_it)
