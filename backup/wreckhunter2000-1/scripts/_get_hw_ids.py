import paramiko
c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('10.0.0.56', username='cesarops', password='cesarops', timeout=10)
def r(cmd, t=15):
    _, o, e = c.exec_command(cmd, timeout=t)
    return (o.read()+e.read()).decode(errors='replace').strip()

print('[nic_mac]')
print(r('ip link show | grep -E "ether"'))
print()
print('[armor_uuid]')
print(r('echo cesarops | sudo -S blkid /dev/sdc1 /dev/sdc2 2>&1'))
print()
print('[xeon_mac]')
# also grab xeon mac for config
c2 = paramiko.SSHClient()
c2.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c2.connect('10.0.0.129', username='cesarops1', password='cesarops1', timeout=10)
def r2(cmd, t=15):
    _, o, e = c2.exec_command(cmd, timeout=t)
    return (o.read()+e.read()).decode(errors='replace').strip()
print(r2('ip link show | grep -E "ether"'))
print()
print('[samba_i7]')
print(r('systemctl is-active smbd 2>/dev/null; cat /etc/samba/smb.conf 2>/dev/null | head -60'))
c.close()
c2.close()
