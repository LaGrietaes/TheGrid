# Windows Security Bypass Guide (Error 4551)

If you are receiving **Error 4551**, it means Windows is blocking the application because of a **System Integrity Policy** (WDAC or SmartApp Control). Here is how to unblock it for development.

## 1. Unblock the Folder (The Master Fix)
Run this in an **Administrator PowerShell** to recursively unblock all files in your project:
```powershell
dir -Path "C:\TheGrid" -Recurse | Unblock-File
```

## 2. Add Windows Defender Exclusion
Run this to prevent the antivirus from scanning/blocking the build artifacts:
```powershell
Add-MpPreference -ExclusionPath "C:\TheGrid"
```

## 3. Disable SmartApp Control (Windows 11)
If you are on Windows 11, this is likely the main blocker:
1. Open **Windows Security**.
2. Go to **App & browser control**.
3. Select **Reputation-based protection settings**.
4. Set **SmartApp Control** to **Off** (or Evaluation).

## 4. Execution Policy
Ensure PowerShell can run the build scripts:
```powershell
Set-ExecutionPolicy -ExecutionPolicy Bypass -Scope Process -Force
```

## 5. Move to a Trusted Path
Windows often trusts files more when they are in your **User Profile**. Try moving the project to:
`C:\Users\Lag-d\Documents\TheGrid`
