import paramiko
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect('100.105.77.74', username='cesarops', password='cesarops')
stdin, stdout, stderr = client.exec_command('cat /home/cesarops/wreckhunter2000-1/run_job_remote.py | grep -i argparse -A 10')
print('OUT:', stdout.read().decode())
