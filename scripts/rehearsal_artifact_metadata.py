"""Describe the one-shot rehearsal's existing metadata gate, without doing I/O.

Callers must retain this report before rejecting metadata. Passing this gate is
not artifact verification: opening safely, hashing bytes, checking stability and
enforcing runner isolation remain the caller's responsibility. This module does
not authorize or start another build.
"""

import os
import stat


MAX_ARTIFACT_BYTES = 4 * 1024 * 1024


def describe_artifact_metadata(metadata: os.stat_result) -> dict:
    """Report every predicate from the failed rehearsal's combined assertion."""
    checks = {
        "regular_file": stat.S_ISREG(metadata.st_mode),
        "single_link": metadata.st_nlink == 1,
        "nonempty": metadata.st_size > 0,
        "within_size_limit": metadata.st_size <= MAX_ARTIFACT_BYTES,
    }
    return {
        "metadata_accepted": all(checks.values()),
        "mode": oct(metadata.st_mode),
        "link_count": metadata.st_nlink,
        "size_bytes": metadata.st_size,
        "max_size_bytes": MAX_ARTIFACT_BYTES,
        "checks": checks,
        "failed_checks": [name for name, passed in checks.items() if not passed],
    }
