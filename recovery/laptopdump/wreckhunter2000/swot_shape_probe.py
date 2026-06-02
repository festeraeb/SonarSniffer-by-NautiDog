import h5py, numpy as np, json, math, requests, tempfile, os
from pathlib import Path

token = json.loads(open('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json').read())['earthdata_token']
session = requests.Session()
session.headers['Authorization'] = f'Bearer {token}'
session.headers['User-Agent'] = 'WreckHunter2000/1.0'

url = ('https://archive.swot.podaac.earthdata.nasa.gov/podaac-swot-ops-cumulus-protected'
       '/SWOT_L2_LR_SSH_2.0/SWOT_L2_LR_SSH_Expert_001_160_20230726T215351_20230726T224519_PGC0_01.nc')

print('Downloading Expert...')
r = session.get(url, timeout=180, stream=True)
tmp = tempfile.NamedTemporaryFile(suffix='.nc', delete=False)
for chunk in r.iter_content(1 << 20):
    tmp.write(chunk)
tmp.close()
print(f'  {Path(tmp.name).stat().st_size/1e6:.1f} MB')

def hav(la1, lo1, la2, lo2):
    R = 6_371_000.0
    p1, p2 = math.radians(la1), math.radians(la2)
    a = (math.sin(math.radians(la2-la1)/2)**2
         + math.cos(p1)*math.cos(p2)*math.sin(math.radians(lo2-lo1)/2)**2)
    return R * 2 * math.asin(math.sqrt(a))

CORRIDOR = [(42.46470, -87.10823), (42.46506, -87.08555), (42.47033, -87.09896),
            (42.46098, -87.09134), (42.45221, -87.07866), (42.47128, -87.08305),
            (42.47732, -87.10543)]

with h5py.File(tmp.name, 'r') as f:
    lat_nadir = np.array(f['latitude_nadir'])   # 1D along-track
    lon_nadir = np.array(f['longitude_nadir'])
    lat_swath = np.array(f['latitude'])          # 2D (num_lines, num_pixels)
    lon_swath = np.array(f['longitude'])
    ssha      = np.array(f['ssha_karin_2'])      # 2D SSH anomaly

    print(f'latitude_nadir shape : {lat_nadir.shape}')
    print(f'latitude (swath) shape: {lat_swath.shape}')
    print(f'ssha_karin_2 shape   : {ssha.shape}')
    print(f'nadir lat range: {float(np.nanmin(lat_nadir)):.2f} to {float(np.nanmax(lat_nadir)):.2f}')
    print(f'nadir lon range: {float(np.nanmin(lon_nadir)):.2f} to {float(np.nanmax(lon_nadir)):.2f}')

    # For each anchor, find closest nadir line first (fast), then scan that line's swath pixels
    print('\nAnchor proximity to this pass:')
    for clat, clon in CORRIDOR:
        # closest nadir line
        nadir_dists = np.array([hav(clat, clon, float(lat_nadir[i]), float(lon_nadir[i]))
                                 for i in range(len(lat_nadir))])
        best_line = int(np.argmin(nadir_dists))
        nadir_dist = float(nadir_dists[best_line])

        # scan swath pixels on ±100 lines around best_line
        lo = max(0, best_line - 100)
        hi = min(lat_swath.shape[0], best_line + 100)
        sub_lat = lat_swath[lo:hi, :]
        sub_lon = lon_swath[lo:hi, :]
        sub_ssh = ssha[lo:hi, :]

        swath_dists = np.full(sub_lat.shape, np.inf)
        for i in range(sub_lat.shape[0]):
            for j in range(sub_lat.shape[1]):
                if not np.isnan(sub_lat[i, j]):
                    swath_dists[i, j] = hav(clat, clon, float(sub_lat[i,j]), float(sub_lon[i,j]))

        min_idx = np.unravel_index(np.argmin(swath_dists), swath_dists.shape)
        min_dist = float(swath_dists[min_idx])
        ssh_val  = float(sub_ssh[min_idx]) if not np.isnan(float(sub_ssh[min_idx])) else None

        print(f'  ({clat:.5f},{clon:.6f})  nadir_dist={nadir_dist/1000:.1f}km  '
              f'swath_dist={min_dist/1000:.2f}km  ssh={ssh_val}')

os.unlink(tmp.name)
