#!/usr/bin/env bash

# 重新压缩 AppImage，并可选生成不打包 WebKit 的 lite 变体
#
# 产物形态:
#   full（默认）: 自带 WebKitGTK / JavaScriptCore / ICU，约 99MiB，自包含、离线可用；
#                 因此需 strip + zstd22 重压缩以控制体积。
#   lite（--lite）: 不打包 WebKitGTK / JavaScriptCore / ICU，改由系统提供
#                 （deb 安装包已在 Depends 中声明 libwebkit2gtk-4.1-0），约 30MiB；
#                 同时移除仅 WebKit 使用的媒体编解码/语音库与 linuxdeploy 误捆绑的孤立 Qt6 库；
#                 启动时 AppRun 会先检查系统库，缺失时弹窗/提示安装并退出。
#   lite 仅作为体积敏感场景的手动下载变体，latest.json 仍只指向 full，
#   应用内自动更新下载的始终是 full 版。
#
# 用法:
#   ./tools/build/appimage-repack.sh <path/to/xxx.AppImage>                 # 原地重打包（full，历史行为）
#   ./tools/build/appimage-repack.sh --lite <path/to/xxx.AppImage>          # 输出 <原名>-lite.AppImage
#   ./tools/build/appimage-repack.sh --lite --out /tmp/x.AppImage <input>   # 指定输出路径
#   ./tools/build/appimage-repack.sh --force --out ... <input>              # 允许覆盖已存在的输出
#
# 原理:
#   复用原 AppImage 的 runtime（文件前 OFFSET 字节，OFFSET 由 ELF 节头表末尾算出），
#   解开 squashfs 后 strip 掉未剥离符号（主要是主程序），再用 zstd 最高级别、1MB 块
#   重新压缩，拼接回 runtime。full 内容不变，只有压缩方式与符号表变化；
#   lite 额外删除内置的 WebKitGTK/JavaScriptCore/ICU 并给 AppRun 注入预检。
#
#   注意：必须用 zstd。Tauri 的 AppImage runtime 只支持 zlib/zstd，用 xz 会导致
#   "Squashfs image uses xz compression, this version supports only zlib, zstd"。
#
# 签名:
#   重打包后原更新签名失效。脚本会用 TAURI_SIGNING_PRIVATE_KEY（内容）或
#   ~/.config/auroradb/updater.key 重新生成 <输出文件>.sig；找不到私钥时只警告。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

LITE_MAX_SIZE=$((40 * 1024 * 1024))

usage() {
    # 打印文件头的注释块（shebang 之后到第一行非注释）
    awk 'NR > 2 && /^#/ { sub(/^# ?/, ""); print; next } NR > 2 { exit }' "$0"
}

fail() { echo -e "${RED}错误: $*${NC}" >&2; exit 1; }

LITE=0
OUT_ARG=""
FORCE=0
IMG=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --lite) LITE=1; shift ;;
        --out)
            [[ $# -ge 2 ]] || fail "--out 需要一个输出路径"
            OUT_ARG="$2"; shift 2
            ;;
        --force) FORCE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        -*) fail "未知参数: $1（可用: --lite --out <path> --force）" ;;
        *)
            [[ -z "$IMG" ]] || fail "只能指定一个输入文件"
            IMG="$1"; shift
            ;;
    esac
done

if [[ -z "$IMG" ]]; then
    usage
    exit 1
fi
if [[ ! -f "$IMG" ]]; then
    fail "文件不存在 $IMG"
fi
IMG="$(cd "$(dirname "$IMG")" && pwd)/$(basename "$IMG")"

for tool in unsquashfs mksquashfs python3 strip; do
    command -v "$tool" >/dev/null || fail "未找到 $tool"
done

# 默认输出：full 原地覆盖；lite 生成 <原名>-lite.AppImage，绝不覆盖输入
if [[ -n "$OUT_ARG" ]]; then
    OUT_DIR="$(cd "$(dirname "$OUT_ARG")" 2>/dev/null && pwd)" || fail "输出目录不存在: $(dirname "$OUT_ARG")"
    OUT="$OUT_DIR/$(basename "$OUT_ARG")"
elif (( LITE )); then
    if [[ "$IMG" == *.AppImage ]]; then
        OUT="${IMG%.AppImage}-lite.AppImage"
    else
        OUT="$IMG-lite"
    fi
else
    OUT="$IMG"
fi

if (( LITE )) && [[ "$OUT" == "$IMG" ]]; then
    fail "--lite 不允许覆盖输入文件，请用 --out 指定其他路径"
fi
if [[ "$OUT" != "$IMG" ]]; then
    [[ -d "$(dirname "$OUT")" ]] || fail "输出目录不存在: $(dirname "$OUT")"
    if [[ -e "$OUT" && $FORCE -eq 0 ]]; then
        fail "输出已存在: $OUT（确认覆盖请加 --force）"
    fi
fi

if (( LITE )); then
    echo -e "${YELLOW}重打包 AppImage (lite): $(basename "$IMG")${NC}"
else
    echo -e "${YELLOW}重打包 AppImage: $(basename "$IMG")${NC}"
fi

# runtime 结束位置 = ELF 节头表末尾（AppImage type 2 的 squashfs 从此处开始）
OFFSET="$(python3 - "$IMG" <<'PY'
import struct, sys

path = sys.argv[1]
with open(path, 'rb') as f:
    head = f.read(64)
    if head[:4] != b'\x7fELF' or head[4] != 2:
        sys.exit('不是 64 位 ELF 文件')
    e_shoff = struct.unpack_from('<Q', head, 0x28)[0]
    e_shentsize = struct.unpack_from('<H', head, 0x3a)[0]
    e_shnum = struct.unpack_from('<H', head, 0x3c)[0]
    if e_shnum == 0:
        f.seek(e_shoff)
        e_shnum = struct.unpack('<Q', f.read(32)[0x20:0x28])[0]
    offset = e_shoff + e_shnum * e_shentsize
    f.seek(offset)
    if f.read(4) != b'hsqs':
        sys.exit('偏移 %d 处不是 squashfs 超级块' % offset)
print(offset)
PY
)" || fail "无法定位 squashfs 偏移"

ORIG_SIZE="$(stat -c %s "$IMG")"
echo "  runtime 偏移: $OFFSET"
echo "  原大小: $(awk "BEGIN{printf \"%.1f\", $ORIG_SIZE/1048576}")MB"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

echo "  解包 squashfs..."
unsquashfs -o "$OFFSET" -d "$WORKDIR/AppDir" "$IMG" >/dev/null

# 主程序带符号（约 9MB），strip 后对可执行文件行为无影响
while IFS= read -r -d '' f; do
    if file -b "$f" | grep -q '^ELF' && file -b "$f" | grep -q 'not stripped'; then
        strip --strip-all "$f" || true
    fi
done < <(find "$WORKDIR/AppDir/usr/bin" -maxdepth 1 -type f -print0 2>/dev/null)

# lite：删除内置 WebKitGTK / JavaScriptCore / ICU，改由系统提供（deb 已声明依赖）
if (( LITE )); then
    echo "  lite: 移除内置 WebKitGTK / JavaScriptCore / ICU..."
    shopt -s nullglob
    PRUNED=(
        "$WORKDIR/AppDir/usr/lib"/libwebkit2gtk-4.1.so*
        "$WORKDIR/AppDir/usr/lib"/libjavascriptcoregtk-4.1.so*
        "$WORKDIR/AppDir/usr/lib"/libicu*.so*
        # WebKitGTK 专属的媒体编解码 / 语音合成库：webkit 删除后仅剩它们自身，
        # 系统 webkit 会从系统包解析这些依赖（否则无法压到 40MiB 以内）。
        "$WORKDIR/AppDir/usr/lib"/libaom.so*
        "$WORKDIR/AppDir/usr/lib"/libavif.so*
        "$WORKDIR/AppDir/usr/lib"/libdav1d.so*
        "$WORKDIR/AppDir/usr/lib"/libflite*.so*
        "$WORKDIR/AppDir/usr/lib"/libgav1.so*
        "$WORKDIR/AppDir/usr/lib"/libjxl.so*
        "$WORKDIR/AppDir/usr/lib"/librav1e.so*
        "$WORKDIR/AppDir/usr/lib"/libSvtAv1Enc.so*
        "$WORKDIR/AppDir/usr/lib"/libyuv.so*
        # linuxdeploy 误捆绑的孤立 Qt6 库（只有 libgio-qt6 引用 libQt6Core，
        # 而 AppDir 内没有任何东西引用 libgio-qt6），Tauri/GTK 应用用不到。
        "$WORKDIR/AppDir/usr/lib"/libQt6*.so*
        "$WORKDIR/AppDir/usr/lib"/libgio-qt6.so*
    )
    shopt -u nullglob
    if (( ${#PRUNED[@]} == 0 )); then
        fail "没有找到可删除的 WebKit/ICU 库（输入可能已是 lite，或 AppDir 结构已变化）"
    fi
    PRUNED_BYTES=0
    for f in "${PRUNED[@]}"; do
        PRUNED_BYTES=$((PRUNED_BYTES + $(stat -c %s "$f")))
    done
    rm -f "${PRUNED[@]}"
    echo "   已删除 ${#PRUNED[@]} 个库，约 $(awk "BEGIN{printf \"%.1f\", $PRUNED_BYTES/1048576}")MB"

    # lite 的 AppRun 需在拉起真正程序前检查系统 WebKitGTK。
    # 插到 exec 之前，保留 linuxdeploy 生成的 gtk hook source 与 set -e。
    echo "  lite: 注入系统 WebKitGTK 预检到 AppRun..."
    APPRUN="$WORKDIR/AppDir/AppRun"
    [[ -f "$APPRUN" ]] || fail "AppDir 中缺少 AppRun"
    PREFLIGHT="$WORKDIR/apprun-lite-preflight.sh"
    cat > "$PREFLIGHT" <<'PREFLIGHT_EOF'
# ---- AuroraDB lite 预检：依赖系统 WebKitGTK（由 tools/build/appimage-repack.sh --lite 注入） ----
_lite_webkit_msg() {
    cat <<'EOF'
AuroraDB 精简版依赖系统 WebKitGTK（libwebkit2gtk-4.1-0），未检测到 libwebkit2gtk-4.1.so.0。

请先安装：
  Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0
  Fedora:        sudo dnf install webkit2gtk4.1

或改用完整版 AppImage / .deb 安装包。
EOF
}

# 优先用 PATH 里的 ldconfig；GUI 环境 PATH 可能不含 /sbin，再回落到绝对路径
_lite_ldconfig() {
    if command -v ldconfig >/dev/null 2>&1; then
        ldconfig -p 2>/dev/null
    elif [[ -x /sbin/ldconfig ]]; then
        /sbin/ldconfig -p 2>/dev/null
    fi
}

if ! _lite_ldconfig | grep -q 'libwebkit2gtk-4.1.so.0'; then
    if command -v zenity >/dev/null 2>&1; then
        zenity --error --no-wrap --title="AuroraDB 精简版" --text="$(_lite_webkit_msg)" 2>/dev/null \
            || _lite_webkit_msg >&2
    elif command -v kdialog >/dev/null 2>&1; then
        kdialog --title "AuroraDB 精简版" --error "$(_lite_webkit_msg)" 2>/dev/null \
            || _lite_webkit_msg >&2
    else
        _lite_webkit_msg >&2
    fi
    exit 1
fi
PREFLIGHT_EOF

    if ! awk -v pre="$PREFLIGHT" '
        !injected && /^exec "\$this_dir"\/AppRun\.wrapped "\$@"$/ {
            while ((getline line < pre) > 0) print line
            close(pre)
            injected = 1
        }
        { print }
        END { if (!injected) exit 3 }
    ' "$APPRUN" > "$WORKDIR/AppRun.new"; then
        fail "AppRun 结构不符合预期（未找到 AppRun.wrapped exec 行），无法注入 lite 预检"
    fi
    grep -qF 'source "$this_dir"/apprun-hooks/' "$WORKDIR/AppRun.new" \
        || fail "AppRun 改写后丢失 linuxdeploy gtk hook，已中止"
    grep -qF 'libwebkit2gtk-4.1.so.0' "$WORKDIR/AppRun.new" \
        || fail "AppRun 改写后未找到 lite 预检内容，已中止"
    chmod 755 "$WORKDIR/AppRun.new"
    mv "$WORKDIR/AppRun.new" "$APPRUN"

    # 验证改成系统库后没有引入其他缺失依赖；webkit/javascriptcore/icu 缺失属预期
    # （构建机可以没装 WebKit，目标机由 AppRun 预检兜底），其他库缺失则直接失败。
    AURORADB="$WORKDIR/AppDir/usr/bin/auroradb"
    [[ -f "$AURORADB" ]] || fail "AppDir 中缺少 usr/bin/auroradb"
    LDD_OUT="$(LC_ALL=C LD_LIBRARY_PATH="$WORKDIR/AppDir/usr/lib:$WORKDIR/AppDir/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        ldd "$AURORADB" 2>/dev/null || true)"
    [[ -n "$LDD_OUT" ]] || fail "ldd 无法分析 usr/bin/auroradb"
    LDD_MISSING="$(awk '/not found/ {print $1}' <<<"$LDD_OUT")"
    if [[ -n "$LDD_MISSING" ]]; then
        LDD_UNEXPECTED="$(grep -Ev '^(libwebkit2gtk-4\.1\.so\.0|libjavascriptcoregtk-4\.1\.so\.0|libicu[a-z0-9]*\.so\.[0-9]+)$' <<<"$LDD_MISSING" || true)"
        if [[ -n "$LDD_UNEXPECTED" ]]; then
            echo -e "${RED}  ✗ lite 版缺少非预期的动态库：${NC}" >&2
            echo "$LDD_UNEXPECTED" | sed 's/^/    /' >&2
            exit 1
        fi
        echo -e "${YELLOW}  ! 构建机未安装系统 WebKit，ldd 缺失: $(echo "$LDD_MISSING" | tr '\n' ' ')（目标机由 AppRun 预检兜底）${NC}"
    else
        echo -e "${GREEN}  ✓ usr/bin/auroradb 动态库解析正常（无 not found）${NC}"
    fi
fi

# runtime 内置的 squashfs reader 只认 zlib/zstd，xz 会导致无法挂载
echo "  重新压缩 squashfs..."
mksquashfs "$WORKDIR/AppDir" "$WORKDIR/root.squashfs" \
    -comp zstd -Xcompression-level 22 -b 1M -noappend -no-progress -all-root >/dev/null

head -c "$OFFSET" "$IMG" > "$WORKDIR/new.AppImage"
cat "$WORKDIR/root.squashfs" >> "$WORKDIR/new.AppImage"
chmod +x "$WORKDIR/new.AppImage"

NEW_SIZE="$(stat -c %s "$WORKDIR/new.AppImage")"
# full 版体积只做提示，GitHub 单附件上限 2GB，不再硬性拦截；
# lite 仍限制在 40MiB 内，保证「体积敏感场景」的定位不被破坏
if (( LITE )) && (( NEW_SIZE > LITE_MAX_SIZE )); then
    echo -e "${RED}  ✗ 重打包后仍有 $(awk "BEGIN{printf \"%.1f\", $NEW_SIZE/1048576}")MB，超过 lite 版 40MiB 上限${NC}"
    exit 1
fi

# 拼接结果必须是 runtime + 可识别的 squashfs
unsquashfs -s -o "$OFFSET" "$WORKDIR/new.AppImage" >/dev/null || {
    echo -e "${RED}  ✗ 重打包结果不是有效的 AppImage${NC}"
    exit 1
}

mv "$WORKDIR/new.AppImage" "$OUT"
echo -e "${GREEN}  ✓ $(awk "BEGIN{printf \"%.1f\", $ORIG_SIZE/1048576}")MB → $(awk "BEGIN{printf \"%.1f\", $NEW_SIZE/1048576}")MB${NC}"
if (( LITE )); then
    echo "  lite 输出: $OUT"
    echo "  依赖系统 WebKitGTK（libwebkit2gtk-4.1-0）；latest.json 仍指向 full，lite 仅作手动下载"
fi

# 重打包改变了文件内容，必须重新签名，否则自动更新校验失败
sign_artifact() {
    local target="$1"
    local sign_key_file="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.config/auroradb/updater.key}"
    rm -f "$target.sig"
    if ! command -v cargo >/dev/null; then
        echo -e "${YELLOW}  ! 未找到 cargo，跳过签名${NC}"
    elif [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
        cargo tauri signer sign -p "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" "$target" >/dev/null
        echo -e "${GREEN}  ✓ 已重新签名 (TAURI_SIGNING_PRIVATE_KEY)${NC}"
    elif [[ -f "$sign_key_file" ]]; then
        cargo tauri signer sign -f "$sign_key_file" -p "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" "$target" >/dev/null
        echo -e "${GREEN}  ✓ 已重新签名 ($sign_key_file)${NC}"
    else
        echo -e "${YELLOW}  ! 未找到更新签名私钥，跳过签名（自动更新将不可用）${NC}"
    fi
}

sign_artifact "$OUT"
