---
description: How to update the headless node on a tablet (Termux)
---
// turbo-all
1. **On PC (`dev-0n3`)**:
   Run `git push origin main` to upload the latest consolidated code.

2. **On Tablet (Termux)**:
   Stop the currently running node (Ctrl+C).

3. **Pull latest code (Clean Sync)**:
   If you have "divergent branches" errors, use this to force the tablet to match the PC exactly:
   ```bash
   cd ~/thegrid-workspace
   git fetch origin
   git reset --hard origin/main
   ```

   *Alternatively, if you want to keep local changes:* `git pull --rebase origin main`

4. **Rebuild the node**:
   ```bash
   cargo build --release -p thegrid-node
   ```

5. **Start the node**:
   ```bash
   ./target/release/thegrid-node
   ```
