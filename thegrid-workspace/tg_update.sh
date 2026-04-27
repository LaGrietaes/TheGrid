#!/data/data/com.termux/files/usr/bin/bash
exec > /sdcard/tg_update.log 2>&1
echo "=== TheGrid Node Update $(date) ==="
echo "HOME=$HOME"
ls -la "$HOME"

# Find the workspace
echo "--- searching for Cargo.toml ---"
REPO=""
for P in "$HOME/TheGrid/thegrid-workspace" "$HOME/thegrid-workspace" "$HOME/TheGrid" "$HOME/apps/TheGrid/thegrid-workspace" "$HOME/apps/thegrid-workspace" "$HOME/apps/TheGrid"; do
  if [ -f "$P/Cargo.toml" ]; then
    REPO="$P"
    break
  fi
done

if [ -z "$REPO" ]; then
  echo "--- deep search ---"
  REPO=$(find "$HOME" -name "Cargo.toml" -not -path "*/target/*" -not -path "*/.cargo/*" 2>/dev/null | head -1 | xargs -I{} dirname {} 2>/dev/null)
fi

if [ -z "$REPO" ]; then
  echo "--- cloning repo ---"
  cd "$HOME"
  git clone https://github.com/LaGrietaes/TheGrid.git TheGrid
  REPO="$HOME/TheGrid/thegrid-workspace"
fi

echo "REPO=$REPO"
if [ ! -f "$REPO/Cargo.toml" ]; then
  echo "ERROR: Repo has no Cargo.toml: $REPO"
  exit 1
fi

echo "REPO=$REPO"
cd "$REPO"
echo "--- git pull ---"
git pull origin main
echo "--- cargo build ---"
cargo build --release -p thegrid-node
echo "--- restarting node ---"
pkill -f thegrid-node 2>/dev/null || true
sleep 1
nohup "$REPO/target/release/thegrid-node" --skip-update-check > /sdcard/tg_node.log 2>&1 &
echo "NODE_PID=$!"
echo "=== DONE ==="

# Enable external apps for future use
mkdir -p "$HOME/.termux"
echo "allow-external-apps = true" > "$HOME/.termux/termux.properties"
echo "external-apps-enabled"
