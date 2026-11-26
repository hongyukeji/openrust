# Fixed redirect oracle

`prepare.py OUTPUT_DIR` downloads the pinned League oauth2-server source and the
three actual URI dependencies, verifying the recorded source/archive SHA-256.
It writes only to the explicitly supplied directory. Run with Python 3, then:

```sh
docker run --rm --network none -v OUTPUT_DIR:/work:ro -w /work \
  php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9 \
  php oracle.php > redirect_oracle.json
```

Replace OUTPUT_DIR with the absolute output directory. The output is the fixture
at `../../fixtures/redirect_oracle.json`: PHP 8.5.12, 1369 rows, including 46
exceptions. The generator invokes the real League validator and URI classes;
it does not reimplement their result in PHP. Altered checked League source must
fail before producing rows. URI packages retain their upstream license files
in the prepared directory. This is a separately pinned compatible protocol
reference, not a claim about a new Composer resolution for Passport.
