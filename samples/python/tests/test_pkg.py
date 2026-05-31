"""Test file — present so test-filtering behaviour can be inspected. Its
imports are DETECTED like any other (test_pkg → a, test_pkg → b)."""

from pkg import a, b  # grouped relative-of-package import — DETECTED.


def test_alpha_beta():
    assert a.alpha() >= b.beta()
