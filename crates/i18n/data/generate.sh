#!/usr/bin/env bash
# Regenerates the number and plural data in this directory from Unicode CLDR, for the languages
# the craft-i18n catalogs and the apps ship (each with its regional variants).
#
# Needs `cargo install icu4x-datagen --version 2.3.0` (the ICU4X version in Cargo.toml) and
# python3. Downloads CLDR's JSON release once into $CACHE.
set -euo pipefail

CLDR=48.2.1
LANGUAGES=(en ar bg cs da de el es et fa fi fr hi hr hu id it ja ka kk ko lt nl pl pt ru sl sr sv te tr uk vi zh)
HERE=$(cd "$(dirname "$0")" && pwd)
CACHE=${CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/craft-i18n-data}
mkdir -p "$CACHE"

# `sr-Latn` and `zh-Hant` are named as well: their CLDR parent is the root, not `sr` or `zh`.
TMPDIR=$CACHE icu4x-datagen --format blob --overwrite \
    --cldr-tag "$CLDR" \
    --markers DecimalSymbolsV1 DecimalDigitsV1 PluralsCardinalV1 \
    --locales "${LANGUAGES[@]}" sr-Latn zh-Hant \
    --out "$HERE/numbers.postcard"

ZIP=$CACHE/icu4x-source-cache/github.com/unicode-org/cldr-json/releases/download/$CLDR/cldr-$CLDR-json-full.zip
python3 -I "$HERE/percent.py" "$ZIP" "$CLDR" "${LANGUAGES[@]}" > "$HERE/locales.rs"
