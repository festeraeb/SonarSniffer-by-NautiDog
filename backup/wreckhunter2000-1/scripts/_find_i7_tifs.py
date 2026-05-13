import paramiko
c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('10.0.0.56', username='cesarops', password='cesarops', timeout=10)
def r(cmd, t=15):
    _, o, e = c.exec_command(cmd, timeout=t)
    return (o.read()+e.read()).decode(errors='replace').strip()

print('[tif_find]', r('find /home/cesarops /mnt -name "*.tif" 2>/dev/null | head -20', 30))
print('[downloads_ls]', r('ls -lh ~/downloads/ 2>/dev/null | head -20'))
print('[du]', r('du -sh ~/downloads/* 2>/dev/null | head -20'))
c.close()
