"""Mount i7 ArmorATD (/mnt/data-external) on Xeon via sshfs."""
import paramiko, time

def conn(host, user, pw):
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(host, username=user, password=pw, timeout=10)
    return c

def r(c, cmd, timeout=30):
    _, o, e = c.exec_command(cmd, timeout=timeout)
    return (o.read() + e.read()).decode(errors='replace').strip()

xeon = conn('10.0.0.129', 'cesarops1', 'cesarops1')
i7   = conn('10.0.0.56',  'cesarops',  'cesarops')

# Step 1: install sshfs on Xeon
print('[sshfs_install]', r(xeon, 'echo cesarops1 | sudo -S apt-get install -y sshfs 2>&1 | tail -3', 90))

# Step 2: generate SSH key on Xeon if not present
r(xeon, 'test -f ~/.ssh/id_rsa || ssh-keygen -t rsa -N "" -f ~/.ssh/id_rsa', 15)
xeon_pubkey = r(xeon, 'cat ~/.ssh/id_rsa.pub')
print('[xeon_pubkey]', xeon_pubkey[:60], '...')

# Step 3: authorize Xeon key on i7
r(i7, 'mkdir -p ~/.ssh && chmod 700 ~/.ssh')
r(i7, 'echo "' + xeon_pubkey + '" >> ~/.ssh/authorized_keys')
r(i7, 'sort -u ~/.ssh/authorized_keys -o ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys')
print('[i7_auth_lines]', r(i7, 'wc -l ~/.ssh/authorized_keys'))

# Step 4: create mount point, unmount if stale, then mount
r(xeon, 'echo cesarops1 | sudo -S mkdir -p /mnt/i7-armor', 15)
r(xeon, 'echo cesarops1 | sudo -S chown cesarops1:cesarops1 /mnt/i7-armor', 10)
r(xeon, 'fusermount -u /mnt/i7-armor 2>/dev/null; true', 10)

mount_cmd = (
    'sshfs -o StrictHostKeyChecking=no,reconnect,ServerAliveInterval=15 '
    'cesarops@10.0.0.56:/mnt/data-external /mnt/i7-armor'
)
result = r(xeon, mount_cmd, 25)
print('[mount]', result or 'ok')
time.sleep(2)

# Step 5: verify
print('[df]', r(xeon, 'df -h /mnt/i7-armor'))
tif_count = r(xeon, 'find /mnt/i7-armor -name "*.tif" -maxdepth 4 | wc -l')
print('[tif_count]', tif_count)

# Step 6: symlink into repo downloads so scan finds them
print('[symlinks]', r(xeon, (
    'ln -sfn /mnt/i7-armor/michigan ~/wreckhunter2000-1/downloads/michigan 2>/dev/null; '
    'ln -sfn /mnt/i7-armor/superior ~/wreckhunter2000-1/downloads/superior 2>/dev/null; '
    'ln -sfn /mnt/i7-armor/huron    ~/wreckhunter2000-1/downloads/huron    2>/dev/null; '
    'ln -sfn /mnt/i7-armor/erie     ~/wreckhunter2000-1/downloads/erie     2>/dev/null; '
    'echo ok'
)))
print('[total_tifs_via_repo]', r(xeon, 'find ~/wreckhunter2000-1/downloads -name "*.tif" | wc -l'))

i7.close()
xeon.close()
print('DONE')
