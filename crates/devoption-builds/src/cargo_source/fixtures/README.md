These objects are synthetic. `root.tree` references the registry adapter's
standalone manifest/lockfile fixtures and the empty Git tree at `src`. That
subtree is deliberately not consumed by the root membership verifier.

`root.commit` and the binary tree were generated with Python's `hashlib.sha1`
over `type + b" " + ascii_decimal_length + b"\0" + payload`, without Git.
The test pins the independently calculated commit and tree IDs and commit
content SHA-256. Neither fixture came from a registered repository.

`sha-mbles-1.bin` is the 640-byte synthetic SHA-1 collision attack vector from
the pinned `sha1collisiondetection` 0.3.4 crate's `test/sha-mbles-1.bin`.
Its MIT license is retained in `collision-LICENSE.txt`. Upstream:
https://gitlab.com/sequoia-pgp/sha1collisiondetection
It checks the shared collision-rejecting hash helper, not Git framing.
