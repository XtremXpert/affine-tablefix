# affine-tablefix

AFFiNE 0.27.4 avec un correctif du crate `affine_doc_loader` (0.1.7) : les tableaux écrits
depuis du Markdown (outils MCP `create_document` / `update_document`) stockaient chaque
cellule en chaîne simple et l'ordre des lignes/colonnes en entiers (`000000`), d'où des
cellules vides dans l'éditeur et l'erreur `invalid order key head: 0` à l'ajout d'une
colonne — [toeverything/AFFiNE#15466](https://github.com/toeverything/AFFiNE/issues/15466).

- `affine_doc_loader/` : crate 0.1.7 de crates.io + correctif (`src/write/builder.rs`,
  `src/schema.rs`, `src/lib.rs`) et test de régression `tests/table_cells.rs`.
- `.github/workflows/build.yml` : compile `affine_server_native` de AFFiNE v0.27.4 avec
  `[patch.crates-io]` vers ce crate (conteneur `rust:bookworm`, glibc 2.36 comme l'image
  officielle), puis publie `ghcr.io/xtremxpert/affine:0.27.4-tablefix`, qui ne remplace que
  `/app/dist/server-native.x64.node` dans l'image officielle.

Le module est compilé sans `AFFINE_PRO_PUBLIC_KEY` / `AFFINE_PRO_LICENSE_AES_KEY`
(secrets du CI officiel) : sans effet pour une instance auto-hébergée sans licence Team.
