import pytest
from the_block import ErrFeeOverflow, ErrInvalidSelector, fee_decompose


def test_fee_split_cases():
    assert fee_decompose(0, 7) == (7, 0)
    assert fee_decompose(1, 4) == (0, 4)
    assert fee_decompose(2, 3) == (2, 1)


def test_fee_errors():
    with pytest.raises(ErrInvalidSelector):
        fee_decompose(3, 1)

    with pytest.raises(ErrFeeOverflow):
        fee_decompose(0, 1 << 63)
