#!/bin/bash
# Reference-theme installer helper (theme-packages T5).
#
# Copies the shipped reference themes from `assets/themes/<Name>/` into the
# user's config themes dir WITHOUT clobbering: a theme the user already has is
# skipped, never overwritten and never deleted. Sourced by install.sh and by the
# scripts-contract tests, which is why it lives under assets/scripts/.
#
# hve_install_reference_themes <src_root> <dest_root>
#   <src_root>   assets/themes in the repository
#   <dest_root>  <config>/hve/themes
#
# Prints one line per theme: "installed <Name>" or "kept <Name>". Returns 0
# when every theme was installed or skipped, non-zero on a copy failure.
hve_install_reference_themes() {
    _hve_themes_src="$1"
    _hve_themes_dest="$2"
    [ -d "$_hve_themes_src" ] || return 0
    mkdir -p "$_hve_themes_dest" || return 1
    for _hve_theme_src in "$_hve_themes_src"/*/; do
        [ -d "$_hve_theme_src" ] || continue
        _hve_theme_name="$(basename "$_hve_theme_src")"
        if [ -e "$_hve_themes_dest/$_hve_theme_name" ]; then
            printf 'kept %s\n' "$_hve_theme_name"
            continue
        fi
        cp -r "$_hve_theme_src" "$_hve_themes_dest/$_hve_theme_name" || return 1
        printf 'installed %s\n' "$_hve_theme_name"
    done
}
