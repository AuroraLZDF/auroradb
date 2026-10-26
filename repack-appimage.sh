#!/usr/bin/env bash

# 重新压缩 AppImage，使其不超过 Gitee 100MB 单附件上限
#
# 用法:
#   ./repack-appimage.sh path/to/AuroraDB_1.0.0_amd64.AppImage
#
# 原理:
#   复用原 AppImage 的 runtime（文件前 OFFSET 字节，OFFSET 由 ELF 节头表末尾算出），
#   解开 squashfs 后 strip 掉未剥离符号（主要是主程序），再用 xz、1MB 块重新压缩，
#   拼接回 runtime。内容不变，只有压缩方式与符号表变化。
#
# 签名:
#   重打包后原更新签名失效。脚本会用 TAURI_SIGNING_PRIVATE_KEY（内容）或
#   ~/.config/auroradb/updater.key 重新生成 <AppImage>.sig；找不到私钥时只警告。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

MAX_SIZE=$((100 * 1024 * 1024))

if [[ $# -ne 1 ]]; then
    echo "用法: $0 <path/to/xxx.AppImage>"
    exit 1
fi

IMG="$1"
if [[ ! -f "$IMG" ]]; then
    echo -e "${RED}错误: 文件不存在 $IMG${NC}"
    exit 1
fi
IMG="$(cd "$(dirname "$IMG")" && pwd)/$(basename "$IMG")"

for tool in unsquashfs mksquashfs python3 strip; do
    command -v "$tool" >/dev/null || { echo -e "${RED}错误: 未找到 $tool${NC}"; exit 1; }
done

echo -e "${YELLOW}重打包 AppImage: $(basename "$IMG")${NC}"

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
)" || { echo -e "${RED}错误: 无法定位 squashfs 偏移${NC}"; exit 1; }

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

# 各版本 AppImage runtime 都支持 xz，兼容性优于 zstd
echo "  重新压缩 squashfs..."
mksquashfs "$WORKDIR/AppDir" "$WORKDIR/root.squashfs" \
    -comp xz -b 1M -noappend -no-progress -all-root >/dev/null

head -c "$OFFSET" "$IMG" > "$WORKDIR/new.AppImage"
cat "$WORKDIR/root.squashfs" >> "$WORKDIR/new.AppImage"
chmod +x "$WORKDIR/new.AppImage"

NEW_SIZE="$(stat -c %s "$WORKDIR/new.AppImage")"
if (( NEW_SIZE > MAX_SIZE )); then
    echo -e "${RED}  ✗ 重打包后仍有 $(awk "BEGIN{printf \"%.1f\", $NEW_SIZE/1048576}")MB，超过 Gitee 单附件上限${NC}"
    exit 1
fi

# 拼接结果必须是 runtime + 可识别的 squashfs
unsquashfs -s -o "$OFFSET" "$WORKDIR/new.AppImage" >/dev/null || {
    echo -e "${RED}  ✗ 重打包结果不是有效的 AppImage${NC}"
    exit 1
}

mv "$WORKDIR/new.AppImage" "$IMG"
echo -e "${GREEN}  ✓ $(awk "BEGIN{printf \"%.1f\", $ORIG_SIZE/1048576}")MB → $(awk "BEGIN{printf \"%.1f\", $NEW_SIZE/1048576}")MB${NC}"

# 重打包改变了文件内容，必须重新签名，否则自动更新校验失败
rm -f "$IMG.sig"
SIGN_KEY_FILE="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.config/auroradb/updater.key}"
if ! command -v cargo >/dev/null; then
    echo -e "${YELLOW}  ! 未找到 cargo，跳过签名${NC}"
elif [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    cargo tauri signer sign -p "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" "$IMG" >/dev/null
    echo -e "${GREEN}  ✓ 已重新签名 (TAURI_SIGNING_PRIVATE_KEY)${NC}"
elif [[ -f "$SIGN_KEY_FILE" ]]; then
    cargo tauri signer sign -f "$SIGN_KEY_FILE" -p "" "$IMG" >/dev/null
    echo -e "${GREEN}  ✓ 已重新签名 ($SIGN_KEY_FILE)${NC}"
else
    echo -e "${YELLOW}  ! 未找到更新签名私钥，跳过签名（自动更新将不可用）${NC}"
fi
