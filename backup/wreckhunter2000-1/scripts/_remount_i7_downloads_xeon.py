"""Remount i7 downloads (actual TIF location) on Xeon and update scan symlinks."""
import paramiko, time

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('10.0.0.129', username='cesarops1', password='cesarops1', timeout=10)

def r(cmd, t=20):
    _, o, e = c.exec_command(cmd, timeout=t)
    return (o.read() + e.read()).decode(errors='replace').strip()

# Clean up old mounts
r('fusermount -u /mnt/i7-armor 2>/dev/null; true')
r('fusermount -u /mnt/i7-downloads 2>/dev/null; true')

# Create new mount point
r('echo cesarops1 | sudo -S mkdir -p /mnt/i7-downloads')
r('echo cesarops1 | sudo -S chown cesarops1:cesarops1 /mnt/i7-downloads')

# Mount i7's actual downloads dir
mount_cmd = (
    'sshfs -o StrictHostKeyChecking=no,reconnect,ServerAliveInterval=15 '
    'cesarops@10.0.0.56:/home/cesarops/downloads /mnt/i7-downloads'
)
result = r(mount_cmd, 20)
print('[mount]', result or 'ok')
time.sleep(2)

print('[df]', r('df -h /mnt/i7-downloads'))

tif_count = r('find /mnt/i7-downloads -name "*.tif" -maxdepth 5 | wc -l', 30)
print('[tif_count]', tif_count)

# Update symlinks in repo to point at mounted i7 data
r('ln -sfn /mnt/i7-downloads/michigan ~/wreckhunter2000-1/downloads/michigan')
r('ln -sfn /mnt/i7-downloads/superior ~/wreckhunter2000-1/downloads/superior')
r('ln -sfn /mnt/i7-downloads/huron    ~/wreckhunter2000-1/downloads/huron')
r('ln -sfn /mnt/i7-downloads/erie     ~/wreckhunter2000-1/downloads/erie')

repo_tifs = r('find ~/wreckhunter2000-1/downloads -name "*.tif" | wc -l', 30)
print('[repo_tifs]', repo_tifs)

c.close()
print('DONE')
