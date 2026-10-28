# Images

How `terrace-contract` writes the label block into a Dockerfile and checks a built image against
its contract: the labels, the embedded file and the attached referrer.

## The label block

`render --format dockerfile --write Dockerfile` rewrites every `# terrace-config:labels:begin`/`:end`
region in the file in place, and `--check Dockerfile` exits 1 when any region is not the block the
document renders. Write the two marker lines where the labels belong once; `--write` fills them, and
leaves everything outside the regions byte for byte. `image verify --dockerfile` is the same
comparison. A Dockerfile building several images carries one region per stage, and every region is
compared or rewritten. An empty region is a finding on its line; a file with no region, an
unterminated or nested one, or more than 32 is refused.

## The built image

`image verify --image REF` reads the image itself through `oras`, so no `docker inspect` output has
to be captured first. It checks:

- the contract labels of every platform, which must agree with each other and with the document;
- the file at the labelled path inside the image, which must be the document byte for byte;
- the referrer attached to the digest, if there is one, which must be the document byte for byte;
- at least one of those two carriers, since an image publishing neither publishes no contract.

`REF` is fully qualified (`docker.io/library/redis:7`, not `redis:7`); a tag is resolved once and
every check is made against the digest.

- `--oci-layout` reads `REF` as an OCI image layout, a directory or tarball as
  `docker buildx build --output type=oci` writes it, so the image is checked after it is built and
  before it is pushed.
- `--signer IDENTITY` also verifies the keyless `cosign` signature against that identity under the
  GitHub Actions issuer.
- `--labels FILE` is the offline form of the label check, for what `docker inspect` printed.

Exit 1 means the image is wrong, 2 that it could not be read.

## Attaching the contract

`image attach --image REF contract.json` attaches the document to the digest as a referrer of
artifact type `application/vnd.terrace.config-schema.v1+json` and prints the referrer's digest,
which is what `cosign sign` then signs. It is safe to re-run: a referrer already holding the
document byte for byte is reported rather than attached twice, and one holding a different document
is refused. With `--oci-layout` the layout must be a directory, since the referrer is written into
it.
