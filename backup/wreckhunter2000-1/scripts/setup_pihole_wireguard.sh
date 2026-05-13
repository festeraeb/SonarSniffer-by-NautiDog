#!/usr/bin/env bash
# =============================================================================
# Pi-hole + WireGuard VPN — Pi setup script
# Run on the Pi (10.0.0.226) as root or with sudo
#
# What this does:
#   1. Installs WireGuard (kernel module + tools)
#   2. Installs Pi-hole (unattended, lighttpd on port 80)
#   3. Configures Pi-hole to listen on the WireGuard interface only
#      (safe — Pi-hole won't be open to the wider internet)
#   4. Creates WireGuard server config  (vpn0 interface, 10.8.0.1)
#   5. Generates client configs for up to 5 devices
#   6. Adds DDNS cron job (Cloudflare or public-IP logger)
#   7. Enables everything on boot
#
# Usage:
#   scp scripts/setup_pihole_wireguard.sh pi@10.0.0.226:~/
#   ssh pi@10.0.0.226  "chmod +x ~/setup_pihole_wireguard.sh && sudo ~/setup_pihole_wireguard.sh"
#
# After running:
#   - Port forward UDP 51820 → 10.0.0.226 on your router
#   - Copy the .conf files from ~/wg-clients/ to each device
#   - Pi-hole admin UI: http://10.8.0.1/admin  (only accessible over VPN)
# =============================================================================

set -euo pipefail

# ── Config ────────────────────────────────────────────────────────────────────
WG_IFACE="vpn0"
WG_PORT=51820
WG_SERVER_IP="10.8.0.1/24"          # Pi's VPN address
WG_SUBNET="10.8.0.0/24"
DNS_ADDR="10.8.0.1"                  # Pi-hole = the VPN gateway

# Names and IPs for client configs (add/remove as needed)
declare -A CLIENTS=(
    [laptop]="10.8.0.2"
    [phone]="10.8.0.3"
    [tablet]="10.8.0.4"
    [work-laptop]="10.8.0.5"
)

PIHOLE_WEB_PW="${PIHOLE_WEB_PASSWORD:-changeme_pihole}"  # override via env
WGDIR="/etc/wireguard"
CLIENTDIR="$HOME/wg-clients"

# ── Sanity ────────────────────────────────────────────────────────────────────
if [[ $EUID -ne 0 ]]; then
    echo "ERROR: run as root: sudo $0"
    exit 1
fi

LAN_IFACE=$(ip route | awk '/^default/ {print $5; exit}')
echo "▶ Detected LAN interface: $LAN_IFACE"
mkdir -p "$WGDIR" "$CLIENTDIR"
chmod 700 "$WGDIR"

# ── 1. WireGuard ──────────────────────────────────────────────────────────────
echo "▶ Installing WireGuard..."
apt-get update -qq
apt-get install -y wireguard wireguard-tools qrencode

# Generate server keys
wg genkey | tee "$WGDIR/server_private.key" | wg pubkey > "$WGDIR/server_public.key"
chmod 600 "$WGDIR/server_private.key"
SERVER_PRIV=$(cat "$WGDIR/server_private.key")
SERVER_PUB=$(cat "$WGDIR/server_public.key")
echo "  Server pubkey: $SERVER_PUB"

# ── 2. Pi-hole (unattended) ───────────────────────────────────────────────────
echo "▶ Installing Pi-hole (unattended)..."

# Unattended setup file
mkdir -p /etc/pihole
cat > /etc/pihole/setupVars.conf <<PICONF
PIHOLE_INTERFACE=$WG_IFACE
IPV4_ADDRESS=${DNS_ADDR}/24
IPV6_ADDRESS=
QUERY_LOGGING=true
INSTALL_WEB_SERVER=true
INSTALL_WEB_INTERFACE=true
LIGHTTPD_ENABLED=true
CACHE_SIZE=10000
DNS_FQDN_REQUIRED=false
DNS_BOGUS_PRIV=true
DNSMASQ_LISTENING=single
WEBPASSWORD=$(echo -n "${PIHOLE_WEB_PW}" | sha256sum | awk '{print $1}' | tr -d $'\n' | (read -r first; echo -n "$first${PIHOLE_WEB_PW}" | sha256sum | awk '{print $1}'))
BLOCKING_ENABLED=true
PICONF

# Run Pi-hole installer
curl -sSL https://install.pi-hole.net | bash /dev/stdin --unattended

# Tell Pi-hole to only listen on the WireGuard interface (not LAN, not internet)
pihole -a -i "$WG_IFACE"
echo "  Pi-hole installed. Admin password: ${PIHOLE_WEB_PW}"

# ── 3. Enable IP forwarding ───────────────────────────────────────────────────
echo "▶ Enabling IP forwarding (IPv4 + IPv6)..."
cat > /etc/sysctl.d/99-ip-forward.conf <<'SYSCTL'
net.ipv4.ip_forward=1
net.ipv6.conf.all.forwarding=1
SYSCTL
sysctl -p /etc/sysctl.d/99-ip-forward.conf -q

# ── 4. WireGuard server config ────────────────────────────────────────────────
echo "▶ Building WireGuard server config..."

# Generate all client keys first so we can add peers in one shot
declare -A CLIENT_PRIV
declare -A CLIENT_PUB
for name in "${!CLIENTS[@]}"; do
    priv=$(wg genkey)
    CLIENT_PRIV[$name]="$priv"
    CLIENT_PUB[$name]=$(echo "$priv" | wg pubkey)
done

# Write server config
{
cat <<SVRCONF
[Interface]
Address = ${WG_SERVER_IP}
ListenPort = ${WG_PORT}
PrivateKey = ${SERVER_PRIV}
# NAT: route VPN client traffic through LAN
PostUp   = iptables -A FORWARD -i ${WG_IFACE} -j ACCEPT; iptables -t nat -A POSTROUTING -o ${LAN_IFACE} -j MASQUERADE
PostDown = iptables -D FORWARD -i ${WG_IFACE} -j ACCEPT; iptables -t nat -D POSTROUTING -o ${LAN_IFACE} -j MASQUERADE

SVRCONF

for name in "${!CLIENTS[@]}"; do
    ip="${CLIENTS[$name]}"
cat <<PEER
# --- ${name} ---
[Peer]
PublicKey = ${CLIENT_PUB[$name]}
AllowedIPs = ${ip}/32

PEER
done
} > "$WGDIR/${WG_IFACE}.conf"
chmod 600 "$WGDIR/${WG_IFACE}.conf"

# ── 5. Client config files ────────────────────────────────────────────────────
echo "▶ Writing client configs to $CLIENTDIR ..."

# Detect public IP for client DNS endpoint
PUBLIC_IP=$(curl -s --max-time 5 https://ipinfo.io/ip || echo "YOUR_PUBLIC_IP")
echo "  Detected public IP: $PUBLIC_IP  (update Endpoint if DDNS is set)"

for name in "${!CLIENTS[@]}"; do
    ip="${CLIENTS[$name]}"
    conf="$CLIENTDIR/${name}.conf"
    cat > "$conf" <<CLIENTCONF
[Interface]
PrivateKey = ${CLIENT_PRIV[$name]}
Address = ${ip}/32
DNS = ${DNS_ADDR}

[Peer]
PublicKey = ${SERVER_PUB}
Endpoint = ${PUBLIC_IP}:${WG_PORT}
# AllowedIPs = 0.0.0.0/0  → route ALL traffic through home (full tunnel + no ads anywhere)
# AllowedIPs = 10.8.0.0/24 → route only VPN/home subnet (split tunnel)
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
CLIENTCONF
    chmod 600 "$conf"
    echo "  $name  →  $ip  ($conf)"

    # Print QR code for phone/tablet
    if [[ "$name" == "phone" || "$name" == "tablet" ]]; then
        echo ""
        echo "=== QR code for $name (scan in WireGuard app) ==="
        qrencode -t ansiutf8 < "$conf"
        echo ""
    fi
done

# ── 6. DDNS cron (logs public IP hourly) ─────────────────────────────────────
echo "▶ Adding DDNS cron job..."
DDNS_SCRIPT="/home/pi/update_ddns.sh"
cat > "$DDNS_SCRIPT" <<'DDNS'
#!/usr/bin/env bash
# Update DDNS record when public IP changes.
# Supports IONOS Hosting DNS API (recommended) or Cloudflare.
# Set credentials in /home/pi/.env — script sources it automatically.

# Load env vars if present
[ -f /home/pi/.env ] && source /home/pi/.env

IP=$(curl -s --max-time 10 https://ipinfo.io/ip)
PREV=$(cat /tmp/public_ip.txt 2>/dev/null || echo "")
if [[ "$IP" != "$PREV" ]]; then
    echo "$(date -Iseconds)  IP changed: $PREV -> $IP" >> /var/log/ddns.log
    echo "$IP" > /tmp/public_ip.txt

    # ── IONOS DDNS ───────────────────────────────────────────────────────
    # Required in /home/pi/.env:
    #   IONOS_API_KEY="prefix.secret"      (from IONOS Developer Portal)
    #   IONOS_ZONE_ID="<zone-uuid>"
    #   IONOS_RECORD_ID="<record-uuid>"
    #   IONOS_RECORD_NAME="home"            (subdomain, e.g. home.yourdomain.com)
    if [[ -n "${IONOS_API_KEY:-}" && -n "${IONOS_ZONE_ID:-}" && -n "${IONOS_RECORD_ID:-}" ]]; then
        curl -s -X PUT \
            "https://api.hosting.ionos.com/dns/v1/zones/${IONOS_ZONE_ID}/records/${IONOS_RECORD_ID}" \
            -H "X-API-Key: ${IONOS_API_KEY}" \
            -H "Content-Type: application/json" \
            --data "[{\"name\":\"${IONOS_RECORD_NAME:-home}\",\"type\":\"A\",\"content\":\"${IP}\",\"ttl\":300,\"prio\":0,\"disabled\":false}]" \
            >> /var/log/ddns.log
        echo " IONOS updated" >> /var/log/ddns.log

    # ── Cloudflare DDNS (fallback, uncomment + fill in) ──────────────────
    # elif [[ -n "${CF_TOKEN:-}" ]]; then
    #     curl -s -X PUT "https://api.cloudflare.com/client/v4/zones/${CF_ZONE_ID}/dns_records/${CF_RECORD_ID}" \
    #       -H "Authorization: Bearer ${CF_TOKEN}" \
    #       -H "Content-Type: application/json" \
    #       --data "{\"type\":\"A\",\"name\":\"home.yourdomain.com\",\"content\":\"${IP}\",\"ttl\":60}" \
    #       >> /var/log/ddns.log

    else
        echo "  WARNING: no DDNS credentials set — IP logged only" >> /var/log/ddns.log
    fi
fi
DDNS
chmod +x "$DDNS_SCRIPT"
chown pi:pi "$DDNS_SCRIPT"

# Add cron (runs every 10 min as pi user)
CRON_LINE="*/10 * * * * /home/pi/update_ddns.sh"
(crontab -u pi -l 2>/dev/null | grep -v update_ddns; echo "$CRON_LINE") | crontab -u pi -
echo "  DDNS cron added (every 10 min). Log at /var/log/ddns.log"

# ── 7. Start + enable WireGuard ───────────────────────────────────────────────
echo "▶ Starting WireGuard..."
systemctl enable "wg-quick@${WG_IFACE}"
systemctl start  "wg-quick@${WG_IFACE}"
wg show

# ── 8. Tailscale exit node ────────────────────────────────────────────────────
echo "▶ Configuring Tailscale as exit node..."
# Make ethtool UDP GRO fix survive reboots (Tailscale performance recommendation)
cat > /etc/systemd/system/tailscale-gro.service <<'UNIT'
[Unit]
Description=Tailscale UDP GRO optimisation
After=network.target
[Service]
Type=oneshot
ExecStart=/sbin/ethtool -K eth0 rx-udp-gro-forwarding on rx-gro-list off
[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable tailscale-gro
# Apply now (best-effort — skip if ethtool missing)
/sbin/ethtool -K eth0 rx-udp-gro-forwarding on rx-gro-list off 2>/dev/null || true
# Advertise as exit node (requires approval at tailscale.com/admin/machines)
if command -v tailscale &>/dev/null; then
    tailscale up --advertise-exit-node 2>&1 || true
    echo "  ⚠  Approve exit node at tailscale.com/admin/machines → cesarops-node → Edit route settings"
else
    echo "  Tailscale not installed — skipping exit node setup"
fi

echo ""
echo "══════════════════════════════════════════════════════════════════"
echo "  DONE!"
echo ""
echo "  Pi-hole admin:  http://10.8.0.1/admin  (over VPN only)"
echo "  Web password:   ${PIHOLE_WEB_PW}"
echo "  WireGuard port: UDP ${WG_PORT}  ← forward this on your router"
echo ""
echo "  Client configs: $CLIENTDIR/"
echo "  Server pubkey:  $SERVER_PUB"
echo ""
echo "  ⚠  ROUTER: forward UDP ${WG_PORT} → 10.0.0.226 (Pi)"
echo "  ⚠  Update Endpoint in client configs if IP changes"
echo "     (or set up Cloudflare DDNS in update_ddns.sh)"
echo "══════════════════════════════════════════════════════════════════"
