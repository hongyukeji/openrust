"""Full init acceptance from an isolated archive, through the shared Fortify HTTP fixture.

Run with mysql_acceptance.py; start with a feature-free main binary in the exclusive target.
"""
import os
from fortify_install import run

if __name__ == "__main__":
    os.environ["OPENRUST_TEST_INIT_AUTH"] = "1"
    run()
