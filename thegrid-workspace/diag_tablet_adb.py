# Diagnostic tool to ensure port 5555 is open on the tablet
import subprocess
import os

def run(cmd):
    try:
        return subprocess.check_output(cmd, shell=True, stderr=subprocess.STDOUT).decode()
    except Exception as e:
        return str(e)

print("--- ADB Diagnosis ---")
print("Devices:\n", run("adb devices"))
print("Attempting to enable port 5555...")
print(run("adb tcpip 5555"))
print("\nVerifying open ports (using ss):")
print(run("ss -antp | grep 5555"))
print("\nVerifying open ports (using netstat -an):")
print(run("netstat -an | grep 5555"))
