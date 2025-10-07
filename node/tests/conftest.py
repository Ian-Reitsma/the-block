import pytest


def pytest_sessionstart(session):
    pytest.skip("python bindings are disabled until the first-party bridge ships")
