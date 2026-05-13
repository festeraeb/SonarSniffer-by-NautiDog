import paramiko
from paramiko import SSHClient, AutoAddPolicy

jump_client = SSHClient()
jump_client.set_missing_host_key_policy(AutoAddPolicy())
jump_client.connect('100.85.138.4', username='cesarops', key_filename='C:/Users/thomf/.ssh/id_ed25519')
transport = jump_client.get_transport()
dest_addr = ('10.0.0.204', 22)
local_addr = ('127.0.0.1', 22)
channel = transport.open_channel("direct-tcpip", dest_addr, local_addr)

target_client = SSHClient()
target_client.set_missing_host_key_policy(AutoAddPolicy())
target_client.connect('10.0.0.204', username='cesarops', password='cesarops', sock=channel)

cmd = "df -h | grep wreckhunter && cat /etc/fstab | grep wreckhunter"
print("Running:", cmd)
_, stdout, stderr = target_client.exec_command(cmd)

print("OUT:", stdout.read().decode())
print("ERR:", stderr.read().decode())