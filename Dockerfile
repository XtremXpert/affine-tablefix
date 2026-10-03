# AFFiNE 0.27.4 avec le module natif serveur corrigé (tableaux créés par le MCP, toeverything/AFFiNE#15466).
ARG BASE_IMAGE=ghcr.io/toeverything/affine@sha256:b649f5ce2384ffdf13c23bccf81d759e15973d59d0b0058af65d895a84373099
FROM ${BASE_IMAGE}
COPY server-native.x64.node /app/dist/server-native.x64.node
