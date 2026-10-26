# my-terminal 远端 Shell 美化。应用通过 ssh 执行 `sh -s -- <mode>`，从标准输入送入本脚本。
# 脚本内容固定，不拼接任何来自配置或界面的文本。
#
# mode
#   auto     连接后自动执行。远端有 ~/.my-terminal/opt-out 时跳过。
#   apply    设置里点「立即美化」。清除 opt-out 后同 auto。
#   restore  设置里点「恢复备份」。移除 my-terminal 段落，并写入 opt-out。
#
# 改 rc 文件之前总是先备份到 ~/.my-terminal/backups/。rc 文件只追加一个带标记的段落，
# 段落只在文件可读时加载 ~/.my-terminal/colors.bash 或 colors.zsh。
# 输出的 MYTERM-* 行由应用解析，其余输出忽略。

umask 077
mode=${1:-auto}
dir="$HOME/.my-terminal"
backups="$dir/backups"
state="$dir/state"
BEGIN='# >>> my-terminal shell colors >>>'
END='# <<< my-terminal shell colors <<<'
HL_TAG=0.8.0
HL_URL=https://github.com/zsh-users/zsh-syntax-highlighting.git

say() { printf 'MYTERM-%s %s\n' "$1" "$2"; }
tilde() {
  case $1 in
    "$HOME"/*) printf '~/%s' "${1#"$HOME"/}" ;;
    *) printf '%s' "$1" ;;
  esac
}
fail() {
  say NOTE "$1"
  say RESULT failed
  exit 1
}

if [ -z "${HOME:-}" ] || [ ! -d "$HOME" ]; then
  fail "远端没有可用的 HOME 目录"
fi
case $mode in
  auto | apply | restore) ;;
  *) fail "未知操作：$mode" ;;
esac

stamp=$(date +%Y%m%d-%H%M%S 2>/dev/null) || stamp=backup
[ -n "$stamp" ] || stamp=backup

has_block() {
  [ -f "$1" ] && grep -qxF "$BEGIN" "$1" 2>/dev/null
}

# Print $1 without the my-terminal block and the one blank line added before it.
strip_block() {
  awk -v b="$BEGIN" -v e="$END" '
    skip { if ($0 == e) skip = 0; next }
    $0 == b { skip = 1; held = 0; next }
    held { print ""; held = 0 }
    $0 == "" { held = 1; next }
    { print }
    END { if (held) print "" }
  ' "$1"
}

# --- restore -----------------------------------------------------------------

restore_file() {
  f=$1
  name=$(basename "$f")
  st="$state/$name"
  if ! has_block "$f"; then
    rm -f "$st"
    return 1
  fi
  backup=$(sed -n 's/^backup=//p' "$st" 2>/dev/null)
  sum=$(sed -n 's/^sum=//p' "$st" 2>/dev/null)
  if [ -n "$backup" ] && [ -n "$sum" ] && [ "$(cksum < "$f")" = "$sum" ]; then
    # Unchanged since the block was added: put the backup back byte for byte.
    case $backup in
      *.absent) rm -f "$f" ;;
      *)
        if [ -f "$backup" ]; then
          cat "$backup" > "$f" || fail "无法恢复 $(tilde "$f")"
        else
          strip_into "$f"
        fi
        ;;
    esac
  else
    # Edited since: keep those edits and remove only our block.
    strip_into "$f"
    if [ -n "$backup" ] && [ "${backup%.absent}" = "$backup" ]; then
      say NOTE "$(tilde "$f") 在美化后改过，只移除了 my-terminal 段落；备份在 $(tilde "$backup")"
    fi
  fi
  rm -f "$st"
  say FILE "$(tilde "$f")"
  if [ -n "$backup" ] && [ "${backup%.absent}" = "$backup" ]; then
    say BACKUP "$(tilde "$backup")"
  fi
  return 0
}

strip_into() {
  tmp="$dir/.strip.$$"
  strip_block "$1" > "$tmp" || {
    rm -f "$tmp"
    fail "无法处理 $(tilde "$1")"
  }
  cat "$tmp" > "$1" || {
    rm -f "$tmp"
    fail "无法写入 $(tilde "$1")"
  }
  rm -f "$tmp"
}

if [ "$mode" = restore ]; then
  mkdir -p "$dir" || fail "无法创建 $(tilde "$dir")"
  changed=0
  for f in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile" "$HOME/.zshrc"; do
    if restore_file "$f"; then
      changed=1
    fi
  done
  rm -f "$dir/colors.bash" "$dir/colors.zsh"
  rm -rf "$dir/zsh-syntax-highlighting"
  : > "$dir/opt-out"
  if [ "$changed" = 1 ]; then
    say RESULT restored
  else
    say RESULT nothing
  fi
  exit 0
fi

# --- auto / apply ------------------------------------------------------------

if [ "$mode" = apply ]; then
  rm -f "$dir/opt-out"
elif [ -e "$dir/opt-out" ]; then
  say RESULT skipped
  exit 0
fi

shell_name=$(basename "${SHELL:-sh}")
case $shell_name in
  bash | zsh) ;;
  *)
    say NOTE "登录 shell 是 $shell_name，只支持 bash 和 zsh"
    say RESULT unsupported
    exit 0
    ;;
esac

mkdir -p "$dir" "$backups" "$state" || fail "无法创建 $(tilde "$dir")"
chmod 700 "$dir" "$backups" "$state" 2>/dev/null

# Write a helper only when its content changed.
put_helper() {
  target=$1
  tmp="$target.tmp.$$"
  cat > "$tmp" || {
    rm -f "$tmp"
    fail "无法写入 $(tilde "$target")"
  }
  chmod 644 "$tmp" 2>/dev/null
  if cmp -s "$tmp" "$target" 2>/dev/null; then
    rm -f "$tmp"
  else
    mv -f "$tmp" "$target" || fail "无法写入 $(tilde "$target")"
  fi
}

put_helper "$dir/colors.bash" <<'HELPER'
# my-terminal：bash 颜色设置。由 my-terminal 生成并在连接时更新，手改会被覆盖。
# 关闭：在 my-terminal 设置里点「恢复备份」，或删掉 ~/.bashrc 里的 my-terminal 段落。
case $- in *i*) ;; *) return 0 ;; esac
[ -n "${MY_TERMINAL_COLORS-}" ] && return 0
MY_TERMINAL_COLORS=1
case ${TERM-} in "" | dumb) return 0 ;; esac

# ls：GNU 用 dircolors 生成 LS_COLORS，目录、可执行文件、链接、压缩包、图片等各有颜色；
# BSD 和 macOS 用 CLICOLOR。已有的 LS_COLORS 和 ls 别名不动。
if ls --color=auto -d / >/dev/null 2>&1; then
  if [ -z "${LS_COLORS-}" ] && command -v dircolors >/dev/null 2>&1; then
    if [ -r "$HOME/.dircolors" ]; then
      eval "$(dircolors -b "$HOME/.dircolors")"
    else
      eval "$(dircolors -b)"
    fi
  fi
  alias ls >/dev/null 2>&1 || alias ls='ls --color=auto'
else
  export CLICOLOR=1
  [ -n "${LSCOLORS-}" ] || export LSCOLORS=ExGxFxdxCxDxDxhbadacec
fi
if echo x | grep --color=auto x >/dev/null 2>&1; then
  alias grep >/dev/null 2>&1 || alias grep='grep --color=auto'
fi

# 提示符：已经有颜色或在用 starship、powerline 等框架时不改。
__mt_prompt=1
__mt_esc=$(printf '\033')
case ${PS1-} in *'\e['* | *'\033['* | *'\E['* | *'\x1b['* | *"$__mt_esc["*) __mt_prompt= ;; esac
case ${PROMPT_COMMAND-} in *starship* | *powerline* | *_omb_* | *liquidprompt* | *__git_ps1*) __mt_prompt= ;; esac
[ -n "${STARSHIP_SHELL-}" ] && __mt_prompt=
if [ -n "$__mt_prompt" ]; then
  PS1='${debian_chroot:+($debian_chroot)}\[\e[1;32m\]\u@\h\[\e[0m\]:\[\e[1;34m\]\w\[\e[0m\]\$ '
fi
unset __mt_prompt __mt_esc
HELPER

put_helper "$dir/colors.zsh" <<'HELPER'
# my-terminal：zsh 颜色设置。由 my-terminal 生成并在连接时更新，手改会被覆盖。
# 关闭：在 my-terminal 设置里点「恢复备份」，或删掉 ~/.zshrc 里的 my-terminal 段落。
[[ -o interactive ]] || return 0
[[ -n ${MY_TERMINAL_COLORS-} ]] && return 0
MY_TERMINAL_COLORS=1
[[ -z ${TERM-} || ${TERM-} == dumb ]] && return 0

# ls：GNU 用 dircolors 生成 LS_COLORS，BSD 和 macOS 用 CLICOLOR。已有设置不动。
if ls --color=auto -d / >/dev/null 2>&1; then
  if [[ -z ${LS_COLORS-} ]] && (( $+commands[dircolors] )); then
    if [[ -r $HOME/.dircolors ]]; then
      eval "$(dircolors -b "$HOME/.dircolors")"
    else
      eval "$(dircolors -b)"
    fi
  fi
  (( $+aliases[ls] )) || alias ls='ls --color=auto'
else
  export CLICOLOR=1
  [[ -n ${LSCOLORS-} ]] || export LSCOLORS=ExGxFxdxCxDxDxhbadacec
fi
if echo x | grep --color=auto x >/dev/null 2>&1; then
  (( $+aliases[grep] )) || alias grep='grep --color=auto'
fi
# 补全列表按 LS_COLORS 上色。
if [[ -n ${LS_COLORS-} ]] && ! zstyle -m ':completion:*' list-colors '*'; then
  zstyle ':completion:*' list-colors "${(s.:.)LS_COLORS}"
fi

# 提示符：已经有颜色或在用 oh-my-zsh、prezto、powerlevel10k、starship 时不改。
() {
  local esc=$'\e'
  [[ -n ${ZSH-} || -n ${ZPREZTODIR-} || -n ${ZSH_THEME-} || -n ${STARSHIP_SHELL-} ]] && return
  (( $+functions[p10k] || $+functions[prompt_powerlevel10k_setup] )) && return
  [[ $PROMPT == *'%F{'* || $PROMPT == *'%K{'* || $PROMPT == *'%{'* || $PROMPT == *"$esc["* ]] && return
  PROMPT='%B%F{green}%n@%m%f%b:%B%F{blue}%~%f%b%# '
}

# 语法高亮：用系统或 Homebrew 装的 zsh-syntax-highlighting，没有时用 my-terminal 下载的那份。
# 必须最后加载，所以 my-terminal 段落追加在 ~/.zshrc 末尾。
if [[ -z ${ZSH_HIGHLIGHT_VERSION-} ]]; then
  for __mt_hl in \
    /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    /usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    /opt/homebrew/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    /usr/local/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    $HOME/.my-terminal/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh; do
    if [[ -r $__mt_hl ]]; then
      source "$__mt_hl"
      break
    fi
  done
  unset __mt_hl
fi
HELPER

# zsh-syntax-highlighting: use an installed copy, else fetch the pinned tag
# once into ~/.my-terminal. A failed fetch is retried only by "apply".
zsh_highlighting() {
  for p in \
    /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    /usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    /opt/homebrew/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    /usr/local/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh \
    "$dir/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"; do
    [ -r "$p" ] && return 0
  done
  if [ -n "${MY_TERMINAL_NO_FETCH:-}" ]; then
    return 0
  fi
  if [ "$mode" = auto ] && [ -e "$dir/.highlight-failed" ]; then
    return 0
  fi
  if ! command -v git >/dev/null 2>&1; then
    : > "$dir/.highlight-failed"
    say NOTE "远端没有 git，未安装 zsh 语法高亮；装好 zsh-syntax-highlighting 后重新登录即可生效"
    return 0
  fi
  rm -rf "$dir/zsh-syntax-highlighting" "$dir/.highlight-tmp"
  if command -v timeout >/dev/null 2>&1; then
    fetch="timeout 60"
  else
    fetch=
  fi
  if GIT_TERMINAL_PROMPT=0 $fetch git -c advice.detachedHead=false clone -q --depth 1 --branch "$HL_TAG" "$HL_URL" "$dir/.highlight-tmp" >/dev/null 2>&1 \
    && mv "$dir/.highlight-tmp" "$dir/zsh-syntax-highlighting"; then
    rm -f "$dir/.highlight-failed"
    say NOTE "已下载 zsh-syntax-highlighting $HL_TAG 到 ~/.my-terminal"
  else
    rm -rf "$dir/.highlight-tmp"
    : > "$dir/.highlight-failed"
    say NOTE "下载 zsh-syntax-highlighting 失败，未开启 zsh 语法高亮"
  fi
}

block_line() {
  case $1 in
    zsh) printf '%s' 'if [ -r "$HOME/.my-terminal/colors.zsh" ]; then . "$HOME/.my-terminal/colors.zsh"; fi' ;;
    *) printf '%s' 'if [ -n "${BASH_VERSION-}" ] && [ -r "$HOME/.my-terminal/colors.bash" ]; then . "$HOME/.my-terminal/colors.bash"; fi' ;;
  esac
}

# Back up $1, then append the block. Prints nothing; returns 1 if the block is already there.
apply_file() {
  f=$1
  kind=$2
  has_block "$f" && return 1
  name=$(basename "$f")
  backup="$backups/$name.$stamp"
  n=1
  while [ -e "$backup" ] || [ -e "$backup.absent" ]; do
    n=$((n + 1))
    backup="$backups/$name.$stamp-$n"
  done
  if [ -e "$f" ]; then
    cp -p "$f" "$backup" 2>/dev/null || cp "$f" "$backup" || fail "无法备份 $(tilde "$f")，没有改动"
  else
    backup="$backup.absent"
    : > "$backup" || fail "无法记录 $(tilde "$f") 原本不存在"
  fi
  {
    if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then
      printf '\n'
    fi
    printf '\n%s\n%s\n%s\n%s\n' "$BEGIN" '# my-terminal 添加的颜色设置。在 my-terminal 设置里点「恢复备份」可移除。' "$(block_line "$kind")" "$END"
  } >> "$f" || fail "无法写入 $(tilde "$f")"
  printf 'backup=%s\nsum=%s\n' "$backup" "$(cksum < "$f")" > "$state/$name"
  say FILE "$(tilde "$f")"
  say BACKUP "$(tilde "$backup")"
  return 0
}

changed=0
if [ "$shell_name" = zsh ]; then
  zsh_highlighting
  apply_file "$HOME/.zshrc" zsh && changed=1
else
  apply_file "$HOME/.bashrc" bash && changed=1
  # An ssh login runs the first of these files only. If it does not read
  # ~/.bashrc, the block goes into it too.
  login=
  for f in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
    if [ -e "$f" ]; then
      login=$f
      break
    fi
  done
  if [ -n "$login" ] && ! grep -q 'bashrc' "$login" 2>/dev/null; then
    apply_file "$login" bash && changed=1
  fi
fi

if [ "$changed" = 1 ]; then
  say RESULT applied
else
  say RESULT unchanged
fi
exit 0
