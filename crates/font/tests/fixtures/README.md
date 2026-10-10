SPDX-License-Identifier: AGPL-3.0-or-later

These compact TTF/OTF/TTC fixtures were generated in-house with FontTools for
atelier-font parser tests. They contain synthetic outlines/metrics and no
third-party font data. The `.font` suffix is intentional; files include both
sfnt flavors and cmap variants. `fixtures.json` records each fixture's SHA-256
and expected oracle behavior. Tests consume the binary fixtures directly and
do not require Python or FontTools.
