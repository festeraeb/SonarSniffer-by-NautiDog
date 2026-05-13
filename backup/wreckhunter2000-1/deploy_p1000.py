import paramiko
import os
import tarfile

print("Compressing workspace...")
with tarfile.open('workspace.tar.gz', 'w:gz') as tar:
    for root, dirs, files in os.walk('.'):
        if '.venv' in root or '.git' in root or '__pycache__' in root or 'node_modules' in root:
            continue
        for file in files:
            if file.endswith('.py') or file.endswith('.json') or file.endswith('.md') or file.endswith('.sh') or file.endswith('.txt') or file == '.env':
                 tar.add(os.path.join(root, file), arcname=os.path.join(root, file))

print("Connecting to P1000 node...")
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect('100.105.77.74', username='cesarops', password='cesarops')

print("Transferring workspace...")
sftp = client.open_sftp()
sftp.put('workspace.tar.gz', '/home/cesarops/workspace.tar.gz')
sftp.close()

print("Extracting workspace...")
_, stdout, stderr = client.exec_command('mkdir -p ~/wreckhunter2000-1 && tar -xzf ~/workspace.tar.gz -C ~/wreckhunter2000-1 && rm ~/workspace.tar.gz')
print(stdout.read().decode())
print(stderr.read().decode())

print("Installing requirements on node...")
# Install pip dependencies
client.exec_command("echo cesarops | sudo -S apt-get update && echo cesarops | sudo -S apt-get install -y python3-venv python3-pip python3-full")
_, stdout, stderr = client.exec_command('cd ~/wreckhunter2000-1 && python3 -m venv .venv && .venv/bin/pip install -r requirements.txt')
print(stdout.read().decode())
print(stderr.read().decode())

client.close()
print("Done!")