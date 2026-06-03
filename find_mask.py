import sys, glob, os, math
from osgeo import gdal, osr
gdal.UseExceptions()
d = sys.argv[1]
TARGET = (45.87127, -84.58642)
recons = sorted(glob.glob(os.path.join(d, "*_recon.tif")))

def center_lonlat(ds):
    gt = ds.GetGeoTransform(); w = ds.RasterXSize; h = ds.RasterYSize
    cx = gt[0] + gt[1] * w / 2; cy = gt[3] + gt[5] * h / 2
    wkt = ds.GetProjection()
    if not wkt:
        return None
    sr = osr.SpatialReference(); sr.ImportFromWkt(wkt)
    tgt = osr.SpatialReference(); tgt.ImportFromEPSG(4326)
    try:
        sr.SetAxisMappingStrategy(osr.OAMS_TRADITIONAL_GIS_ORDER)
        tgt.SetAxisMappingStrategy(osr.OAMS_TRADITIONAL_GIS_ORDER)
    except Exception:
        pass
    ct = osr.CoordinateTransformation(sr, tgt)
    lon, lat, _ = ct.TransformPoint(cx, cy)
    return lat, lon

def hav(a, b, c, e):
    R = 6371000.0; p1, p2 = math.radians(a), math.radians(c)
    dp = math.radians(c - a); dl = math.radians(e - b)
    x = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * R * math.asin(math.sqrt(x))

rows = []
for r in recons:
    ds = gdal.Open(r)
    ll = center_lonlat(ds)
    if ll is None:
        rows.append((None, os.path.basename(r), None, ds.RasterXSize, ds.RasterYSize))
        continue
    dist = hav(TARGET[0], TARGET[1], ll[0], ll[1])
    rows.append((dist, os.path.basename(r), ll, ds.RasterXSize, ds.RasterYSize))

known = [r for r in rows if r[0] is not None]
known.sort(key=lambda x: x[0])
print("%d recon tiles, %d georeferenced" % (len(recons), len(known)))
print("closest masks to Burns candidate (45.87127,-84.58642):")
for dist, name, ll, w, h in known[:10]:
    print("  %8.0fm  %-32s center=(%.5f,%.5f) %dx%dpx" % (dist, name, ll[0], ll[1], w, h))
