ARG OLLAMA_IMAGE=ollama/ollama@sha256:5d3a2f25f9825628daba94dc0b1d421c7d57d6c03ae5faa9ba050e058bab8241
FROM ${OLLAMA_IMAGE}
COPY --chmod=0555 soapbox /usr/local/bin/soapbox
ENV HOME=/tmp OLLAMA_MODELS=/models OLLAMA_NO_CLOUD=1
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/soapbox", "sandbox-run"]
