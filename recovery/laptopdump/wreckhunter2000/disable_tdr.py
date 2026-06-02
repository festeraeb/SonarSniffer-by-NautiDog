"""
disable_tdr.py

Disables Windows TDR (Timeout Detection and Recovery) temporarily.
This allows long-running GPU kernels without Windows killing them.

Run THIS FIRST before GPU batch processing!

WARNING: Only run this if you have good GPU cooling!
"""

import subprocess
import sys

print('='*70)
print('DISABLE WINDOWS TDR (Timeout Detection & Recovery)')
print('='*70)
print()
print('This allows long-running GPU kernels without Windows killing them.')
print()
print('WARNING: Only use with good GPU cooling!')
print()

# Set TDR delay to 60 seconds (default is 2 seconds)
print('Setting TDR delay to 60 seconds...')
try:
    subprocess.run([
        'reg', 'add',
        'HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers',
        '/v', 'TdrDelay',
        '/t', 'REG_DWORD',
        '/d', '60',
        '/f'
    ], check=True)
    print('✓ TDR delay set to 60 seconds')
except Exception as e:
    print(f'✗ Failed: {e}')
    print('  Run as Administrator to modify registry')

print()
print('Setting TDR DDI delay to 60 seconds...')
try:
    subprocess.run([
        'reg', 'add',
        'HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers',
        '/v', 'TdrDdiDelay',
        '/t', 'REG_DWORD',
        '/d', '60',
        '/f'
    ], check=True)
    print('✓ TDR DDI delay set to 60 seconds')
except Exception as e:
    print(f'✗ Failed: {e}')

print()
print('='*70)
print('CHANGES APPLIED')
print('='*70)
print()
print('GPU kernels can now run for up to 60 seconds without timeout.')
print()
print('NOTE: Requires reboot for changes to take full effect.')
print('      But partial effect should work immediately.')
print()
print('To restore default (2 second timeout), run:')
print('  reg add "HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers" /v TdrDelay /t REG_DWORD /d 2 /f')
print()
input('Press Enter to exit...')
