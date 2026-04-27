#!/data/data/com.termux/files/usr/bin/bash
# Probe proot Debian models directory
PROOT_MODELS="$PREFIX/var/lib/proot-distro/installed-rootfs/debian/root/models"
OUT=/sdcard/proot_probe.txt
{
  echo "=== proot Debian models dir ==="
  ls -lah "$PROOT_MODELS/" 2>&1
  echo ""
  echo "=== Total ==="
  du -sh "$PROOT_MODELS/" 2>&1
  echo ""
  echo "=== Find all .gguf/.bin files in proot ==="
  find "$PREFIX/var/lib/proot-distro/installed-rootfs/debian/root/" -type f \( -name "*.gguf" -o -name "*.bin" \) -exec ls -lah {} \; 2>&1 | grep -v "\.so\."
  echo ""
  echo "=== proot-distro installed distros ==="
  ls "$PREFIX/var/lib/proot-distro/installed-rootfs/" 2>&1
  echo ""
  echo "=== llama-server path ==="
  which llama-server 2>/dev/null || ls "$HOME/llama.cpp/build/bin/llama-server" 2>&1
} > "$OUT"
echo "DONE: $OUT"
