"""
Configure i7 for permanent ArmorATD hosting:
  1. udev rule to auto-mount ArmorATD at /mnt/cesarops-armor by partition UUID
  2. Samba share 'cesarops-armor' pointing at that mount
"""
import paramiko

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('10.0.0.56', username='cesarops', password='cesarops', timeout=10)

def r(cmd, t=30):
    _, o, e = c.exec_command(cmd, timeout=t)
    return (o.read() + e.read()).decode(errors='replace').strip()

ARMOR_UUID = "dec00b8b-a95a-4c02-ae40-f7e6ab1b21e9"
MOUNT_POINT = "/mnt/cesarops-armor"

# ── 1. Create mount point ──────────────────────────────────────────────────
print('[mkdir]', r(f'echo cesarops | sudo -S mkdir -p {MOUNT_POINT}'))

# ── 2. udev rule: auto-mount by UUID when drive is plugged in ─────────────
udev_rule = (
    f'ACTION=="add", ENV{{ID_FS_UUID}}=="{ARMOR_UUID}", '
    f'RUN+="/bin/mkdir -p {MOUNT_POINT}", '
    f'RUN+="/bin/mount -U {ARMOR_UUID} {MOUNT_POINT} -o defaults,nofail,uid=1000,gid=1000"'
)
print('[udev]', r(
    f'echo \'{udev_rule}\' | sudo tee /etc/udev/rules.d/99-cesarops-armor.rules'
))
print('[udev_reload]', r('echo cesarops | sudo -S udevadm control --reload-rules && echo ok'))

# ── 3. fstab entry as fallback (nofail so boot succeeds if drive absent) ───
fstab_line = f'UUID={ARMOR_UUID}  {MOUNT_POINT}  ext4  defaults,nofail,x-systemd.device-timeout=5  0  2'
# Only add if not already there
print('[fstab]', r(
    f'grep -q "{ARMOR_UUID}" /etc/fstab || echo \'{fstab_line}\' | sudo tee -a /etc/fstab && echo ok'
))

# ── 4. Mount now ──────────────────────────────────────────────────────────
print('[mount_now]', r(f'echo cesarops | sudo -S mount UUID={ARMOR_UUID} {MOUNT_POINT} 2>&1 || echo already_mounted'))
print('[verify]', r(f'df -h {MOUNT_POINT}'))

# ── 5. Samba share ────────────────────────────────────────────────────────
smb_share = f"""
[cesarops-armor]
   comment = CESAROPS WreckHunter Data Drive
   path = {MOUNT_POINT}
   browseable = yes
   read only = no
   create mask = 0664
   directory mask = 0775
   valid users = cesarops
   force user = cesarops
"""

# Only add if not already there
check = r('grep -q "cesarops-armor" /etc/samba/smb.conf && echo exists || echo missing')
print('[smb_check]', check)
if 'missing' in check:
    print('[smb_add]', r(f"echo '{smb_share}' | sudo tee -a /etc/samba/smb.conf"))

# Ensure Samba password is set for cesarops
print('[samba_pw]', r('(echo cesarops; echo cesarops) | sudo smbpasswd -a cesarops -s 2>&1 || true'))
print('[smbd_reload]', r('echo cesarops | sudo -S systemctl reload smbd && echo ok'))
print('[smb_test]', r(f'smbclient -L localhost -U cesarops%cesarops -N 2>/dev/null | grep cesarops-armor || echo share_not_listed_yet'))

c.close()
print('DONE')
