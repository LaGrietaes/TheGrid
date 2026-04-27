#!/data/data/com.termux/files/usr/bin/bash
# List llama.cpp contents for inspection
LLAMA="$HOME/llama.cpp"
OUT=/sdcard/llama_probe.txt
{
  echo "=== llama.cpp directory listing ==="
  ls -lah "$LLAMA/" 2>&1
  echo ""
  echo "=== .gguf / .bin files (with sizes) ==="
  find "$LLAMA" -type f \( -name "*.gguf" -o -name "*.bin" \) -exec ls -lah {} \; 2>&1
  echo ""
  echo "=== Total sizes per subdir ==="
  du -sh "$LLAMA"/* 2>&1
  echo ""
  echo "=== Grand total ==="
  du -sh "$LLAMA" 2>&1
  echo "=== llama-server / llama-cli present? ==="
  ls "$LLAMA/build/bin/" 2>/dev/null || echo "no build/bin"
} > "$OUT"
echo "DONE: $OUT"
