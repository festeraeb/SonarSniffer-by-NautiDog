sudo sed -i "/\[GArmor\]/,\$d" /etc/samba/smb.conf
echo "[GArmor]" | sudo tee -a /etc/samba/smb.conf
echo "    path = /mnt/data-external" | sudo tee -a /etc/samba/smb.conf
echo "    browseable = yes" | sudo tee -a /etc/samba/smb.conf
echo "    read only = no" | sudo tee -a /etc/samba/smb.conf
echo "    guest ok = yes" | sudo tee -a /etc/samba/smb.conf
echo "    guest account = nobody" | sudo tee -a /etc/samba/smb.conf
echo "    force user = cesarops" | sudo tee -a /etc/samba/smb.conf
echo "    create mask = 0777" | sudo tee -a /etc/samba/smb.conf
echo "    directory mask = 0777" | sudo tee -a /etc/samba/smb.conf
sudo systemctl restart smbd
