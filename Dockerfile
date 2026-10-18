# Built by .github/workflows/release.yml from the prebuilt static binaries in dist/image/<arch>/.
FROM alpine:3.22
ARG TARGETARCH
# git: new-code detection runs `git merge-base`/`git diff`. The checkout is mounted with another
# owner, so mark every directory safe.
RUN apk add --no-cache git && git config --system --add safe.directory '*'
COPY dist/image/${TARGETARCH}/unc /usr/local/bin/unc
WORKDIR /src
ENTRYPOINT ["unc"]
CMD ["analyze"]
