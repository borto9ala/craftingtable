#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
#  craftingtable — interactive media toolbox
#  Arrow-key menus · Fedora friendly · run it with: ./craftingtable.sh
#  (it's a bash script; running it from zsh is fine, just don't `source` it)
# ──────────────────────────────────────────────────────────────

# ── Colors ────────────────────────────────────────────────────
BOLD=$'\e[1m'; DIM=$'\e[2m'; RESET=$'\e[0m'
RED=$'\e[31m'; GREEN=$'\e[32m'; YELLOW=$'\e[33m'
MAGENTA=$'\e[35m'; CYAN=$'\e[36m'

show_cursor() { printf '\e[?25h'; }
trap show_cursor EXIT
trap 'show_cursor; echo; exit 130' INT

ok()   { printf '%s✔%s %s\n' "$GREEN$BOLD" "$RESET" "$*"; }
err()  { printf '%s✘%s %s\n' "$RED$BOLD" "$RESET" "$*"; }
info() { printf '%s➜%s %s\n' "$CYAN$BOLD" "$RESET" "$*"; }

pause() {
    printf '\n%s  Press any key to return to the menu…%s' "$DIM" "$RESET"
    IFS= read -rsn1 _
}

header() {
    clear
    printf '%s' "$MAGENTA$BOLD"
    if command -v figlet &>/dev/null; then
        figlet -f small "craftingtable" 2>/dev/null || figlet "craftingtable"
    else
        echo "╔══════════════════════════════════════╗"
        echo "║            CRAFTING TABLE            ║"
        echo "╚══════════════════════════════════════╝"
    fi
    printf '%s' "$RESET"
}

# ── Arrow-key menu ────────────────────────────────────────────
# Usage: menu "Title" "opt 1" "opt 2" ...  || return 2
# Result index goes into $MENU_IDX. Returns 1 if cancelled (q / Esc).
MENU_IDX=0
menu() {
    local title="$1"; shift
    local -a opts=("$@")
    local n=${#opts[@]} sel=0 key rest i

    printf '\n%s%s%s\n%s  ↑/↓ move · Enter select · 1-9 quick pick · q/Esc back%s\n' \
        "$BOLD$CYAN" "$title" "$RESET" "$DIM" "$RESET"
    printf '\e[?25l'

    while true; do
        for i in "${!opts[@]}"; do
            if (( i == sel )); then
                printf '\e[2K  %s❯ %s%s\n' "$GREEN$BOLD" "${opts[i]}" "$RESET"
            else
                printf '\e[2K    %s\n' "${opts[i]}"
            fi
        done
        printf '\e[%dA' "$n"

        IFS= read -rsn1 key
        if [[ $key == $'\e' ]]; then
            rest=""
            IFS= read -rsn2 -t 0.1 rest
            case $rest in
                '[A') key=UP ;;
                '[B') key=DOWN ;;
                '[H') key=HOME ;;
                '[F') key=END ;;
                *)    key=ESC ;;
            esac
        fi

        case $key in
            UP|k)   (( sel = (sel - 1 + n) % n )) ;;
            DOWN|j) (( sel = (sel + 1) % n )) ;;
            HOME)   sel=0 ;;
            END)    sel=$(( n - 1 )) ;;
            [1-9])  if (( key <= n )); then sel=$(( key - 1 )); break; fi ;;
            '')     break ;;
            q|Q|ESC)
                printf '\e[2A\e[J'; show_cursor
                return 1 ;;
        esac
    done

    printf '\e[2A\e[J'; show_cursor
    MENU_IDX=$sel
    printf '%s✔%s %s%s%s: %s\n' "$GREEN" "$RESET" "$BOLD" "$title" "$RESET" "${opts[sel]}"
    return 0
}

# ── Text prompt (readline → Tab-completes paths) ──────────────
# Usage: ask "Prompt" [default]   → result in $ANS
ask() {
    local prompt="$1" def="$2" suffix=""
    [[ -n $def ]] && suffix=" ${DIM}[$def]${RESET}"
    printf '%s?%s %s%s: ' "$YELLOW$BOLD" "$RESET" "$prompt" "$suffix"
    IFS= read -re ANS
    ANS=${ANS:-$def}
}

# Clean up drag & drop paths: strip quotes, unescape, expand ~
clean_path() {
    local p="$1"
    p="${p#"${p%%[![:space:]]*}"}"; p="${p%"${p##*[![:space:]]}"}"
    p="${p#\'}"; p="${p%\'}"; p="${p#\"}"; p="${p%\"}"
    p=$(sed 's/\\\(.\)/\1/g' <<<"$p")
    [[ $p == "~"* ]] && p="$HOME${p:1}"
    printf '%s' "$p"
}

need() {
    command -v "$1" &>/dev/null && return 0
    err "'$1' is not installed.  →  $2"
    return 1
}

# ── Destination handling ──────────────────────────────────────
get_dest() {
    local input="$1"
    case "$input" in
        dz|"") echo "$HOME/Downloads" ;;
        dc)    echo "$HOME/Documents" ;;
        img)   echo "$HOME/Pictures" ;;
        mv)    echo "$HOME/Videos" ;;
        ms)    echo "$HOME/Music" ;;
        dz/*)  echo "$HOME/Downloads/${input#dz/}" ;;
        dc/*)  echo "$HOME/Documents/${input#dc/}" ;;
        img/*) echo "$HOME/Pictures/${input#img/}" ;;
        mv/*)  echo "$HOME/Videos/${input#mv/}" ;;
        ms/*)  echo "$HOME/Music/${input#ms/}" ;;
        *)     echo "$input" ;;
    esac
}

# Result in $DEST (directory is created). Returns 1 if cancelled.
select_destination() {
    local base
    menu "Destination" \
        "📥  Downloads" "📄  Documents" "🖼️   Pictures" "🎬  Videos" "🎵  Music" "✏️   Custom path…" \
        || return 1
    case $MENU_IDX in
        0) base="$HOME/Downloads" ;;
        1) base="$HOME/Documents" ;;
        2) base="$HOME/Pictures" ;;
        3) base="$HOME/Videos" ;;
        4) base="$HOME/Music" ;;
        5) ask "Path (or shortcut like dz, dc/work, ms/mix)"
           base=$(get_dest "$(clean_path "$ANS")") ;;
    esac
    ask "New subfolder inside it (blank for none)"
    [[ -n $ANS ]] && base="$base/$ANS"
    DEST="$base"
    mkdir -p "$DEST" || { err "Cannot create $DEST"; return 1; }
    return 0
}

# ══════════════════════════════════════════════════════════════
#  Actions   (return 2 = cancelled, so no "press any key" pause)
# ══════════════════════════════════════════════════════════════

do_convert() {
    need ffmpeg "sudo dnf install ffmpeg   (enable RPM Fusion first)" || return 1
    ask "File to convert (drag & drop or Tab-complete)"
    local src; src=$(clean_path "$ANS")
    [[ -f $src ]] || { err "File not found: $src"; return 1; }

    local formats=(mp4 mkv webm mp3 m4a flac wav png jpg webp gif "other…")
    menu "Target format" "${formats[@]}" || return 2
    local ext=${formats[MENU_IDX]}
    if [[ $ext == "other…" ]]; then ask "Extension"; ext=${ANS#.}; fi
    [[ -n $ext ]] || { err "No format given."; return 1; }

    select_destination || return 2

    local filename base out
    filename=$(basename -- "$src"); base="${filename%.*}"
    out="$DEST/$base.$ext"

    info "Converting…"
    if ffmpeg -hide_banner -i "$src" "$out"; then ok "Saved to $out"; else err "ffmpeg failed."; return 1; fi
}

do_scale() {
    need ffmpeg "sudo dnf install ffmpeg   (enable RPM Fusion first)" || return 1
    ask "Video file to scale (drag & drop or Tab-complete)"
    local src; src=$(clean_path "$ANS")
    [[ -f $src ]] || { err "File not found: $src"; return 1; }

    local labels=("720p  (HD)" "1080p (Full HD)" "1440p (2K)" "2160p (4K)" "Custom…")
    local heights=(720 1080 1440 2160)
    menu "Target resolution" "${labels[@]}" || return 2
    local res
    if (( MENU_IDX == 4 )); then ask "Target size in pixels"; res=$ANS; else res=${heights[MENU_IDX]}; fi
    [[ $res =~ ^[0-9]+$ ]] || { err "Invalid resolution."; return 1; }

    menu "Orientation" "Horizontal (landscape)" "Vertical (portrait)" || return 2
    local scale_filter
    if (( MENU_IDX == 0 )); then scale_filter="scale=-2:$res"; else scale_filter="scale=$res:-2"; fi

    select_destination || return 2

    local filename base out
    filename=$(basename -- "$src"); base="${filename%.*}"
    out="$DEST/${base}_${res}p.mp4"

    info "Scaling…"
    if ffmpeg -hide_banner -i "$src" -vf "$scale_filter" -vcodec libx264 -crf 23 -acodec aac "$out"; then
        ok "Saved to $out"
    else
        err "ffmpeg failed."; return 1
    fi
}

do_media() {
    need yt-dlp "sudo dnf install yt-dlp   (or: pipx install yt-dlp)" || return 1
    ask "Paste video link(s) (space-separated)"
    local -a links; read -ra links <<<"$ANS"
    (( ${#links[@]} )) || { err "No links given."; return 1; }

    menu "Download as" \
        "🎬  Video  (mp4)" \
        "🎵  Audio  (mp3 + cover art)" \
        "🎵  Audio  (m4a + cover art)" || return 2
    local kind=$MENU_IDX

    # Cookies for login-protected sites
    local -a cookie=()
    if [[ "${links[*]}" =~ (instagram\.com|facebook\.com|x\.com|twitter\.com) ]]; then
        info "Login-protected site detected."
        local browsers=(none firefox chrome brave librewolf chromium)
        menu "Grab cookies from browser" "${browsers[@]}" || return 2
        (( MENU_IDX > 0 )) && cookie=(--cookies-from-browser "${browsers[MENU_IDX]}")
    fi

    select_destination || return 2

    info "Downloading…"
    if (( kind == 0 )); then
        yt-dlp "${cookie[@]}" -S res,ext:mp4:m4a --recode mp4 -P "$DEST" "${links[@]}"
    else
        local afmt=mp3; (( kind == 2 )) && afmt=m4a
        # Needs ffmpeg for audio extraction + embedding the cover.
        # --convert-thumbnails jpg  → YouTube serves webp, which can't be embedded in mp3/m4a
        # --ppa … crop              → center-crops the 16:9 thumbnail into a square album cover
        #                             (delete the --ppa line if you want the full thumbnail)
        need ffmpeg "sudo dnf install ffmpeg   (enable RPM Fusion first)" || return 1
        yt-dlp "${cookie[@]}" -x --audio-format "$afmt" --audio-quality 0 \
            --embed-thumbnail --convert-thumbnails jpg --embed-metadata \
            --ppa "EmbedThumbnail+ffmpeg_o:-c:v mjpeg -vf crop=\"'if(gt(ih,iw),iw,ih)':'if(gt(iw,ih),ih,iw)'\"" \
            -P "$DEST" "${links[@]}"
    fi
    (( $? == 0 )) && ok "Download complete → $DEST" || { err "yt-dlp reported errors."; return 1; }
}

do_imgconvert() {
    need magick "sudo dnf install ImageMagick" || return 1
    ask "Source folder (drag & drop or Tab-complete)"
    local src_dir; src_dir=$(clean_path "$ANS")
    [[ -d $src_dir ]] || { err "Folder not found: $src_dir"; return 1; }

    ask "Source file extension (e.g. webp, jpg)"
    local src_ext=${ANS#.}
    [[ -n $src_ext ]] || { err "No extension given."; return 1; }

    local fmts=(png jpg webp avif "other…")
    menu "Target format" "${fmts[@]}" || return 2
    local target=${fmts[MENU_IDX]}
    if [[ $target == "other…" ]]; then ask "Extension"; target=${ANS#.}; fi

    menu "Output names" "Keep original names" "Number sequentially (1, 2, 3…)" || return 2
    local naming=$MENU_IDX

    select_destination || return 2

    shopt -s nullglob nocaseglob
    local -a files=("$src_dir"/*."$src_ext")
    shopt -u nullglob nocaseglob
    (( ${#files[@]} )) || { err "No .$src_ext files in $src_dir"; return 1; }

    local i=0 f out fails=0 total=${#files[@]}
    for f in "${files[@]}"; do
        (( i++ ))
        if (( naming == 0 )); then out="$DEST/$(basename "${f%.*}").$target"; else out="$DEST/$i.$target"; fi
        printf '\r\e[2K%s[%d/%d]%s %s' "$CYAN" "$i" "$total" "$RESET" "$(basename "$f")"
        magick "$f" "$out" 2>/dev/null || (( fails++ ))
    done
    echo
    ok "Converted $(( total - fails ))/$total images → $DEST"
    (( fails )) && err "$fails file(s) failed."
    return 0
}

do_gallery() {
    need gallery-dl "sudo dnf install gallery-dl   (or: pipx install gallery-dl)" || return 1
    ask "Paste image/gallery link(s) (space-separated)"
    local -a links; read -ra links <<<"$ANS"
    (( ${#links[@]} )) || { err "No links given."; return 1; }

    select_destination || return 2

    info "Fetching…"
    local link
    for link in "${links[@]}"; do
        gallery-dl -d "$DEST" "$link"
    done
    ok "Done → $DEST"
}

do_fast() {
    need aria2c "sudo dnf install aria2" || return 1
    ask "Paste file download link"
    local url=$ANS
    [[ -n $url ]] || { err "No link given."; return 1; }
    ask "Custom filename (blank = original name)"
    local custom_name=$ANS
    ask "Max speed (e.g. 5M, 500K — blank = unlimited)"
    local speed=$ANS

    select_destination || return 2

    local -a args=(-x 16 -s 16 -d "$DEST")
    [[ -n $custom_name ]] && args+=(-o "$custom_name")
    [[ -n $speed ]]       && args+=(--max-download-limit="$speed")

    info "Launching aria2c…"
    if aria2c "${args[@]}" "$url"; then ok "Saved to $DEST"; else err "aria2c failed."; return 1; fi
}

# ══════════════════════════════════════════════════════════════
#  Main loop
# ══════════════════════════════════════════════════════════════
main() {
    local rc
    while true; do
        header
        menu "What would you like to craft today?" \
            "🔄  Format conversion          (media / images)" \
            "📐  Resolution scaling         (video)" \
            "📹  Mass video/audio download  (YT, TikTok, IG, FB, X…)" \
            "🖼️   Mass image conversion      (magick)" \
            "🌐  Mass image/gallery download (IG, X, Reddit, Rp…)" \
            "🚀  Fast single-file download  (aria2c)" \
            "🚪  Quit" || break

        echo
        case $MENU_IDX in
            0) do_convert ;;
            1) do_scale ;;
            2) do_media ;;
            3) do_imgconvert ;;
            4) do_gallery ;;
            5) do_fast ;;
            6) break ;;
        esac
        rc=$?
        (( rc == 2 )) || pause
    done
    echo "Bye 👋"
}

main
