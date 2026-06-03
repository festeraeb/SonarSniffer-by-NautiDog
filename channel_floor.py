#!/usr/bin/env python3
"""Sample channel-floor (seabed) depth from a reconstruction GeoTIFF.

For a glacial thalweg the floor should trend smoothly. This samples the FLOOR
(local deep percentile, i.e. ignoring any raised object) at given lat/lon
points and along a transect, so a raised feature can be compared against the
surrounding channel grade.

Reports per point:
  floor_p15  : 15th pct depth in window  (the seabed the feature sits on)
  local_max  : shallowest depth in window (top of any raised object)
  relief     : local_max - floor_p15     (how far the object pokes up)
NumPy 2.x safe.
"""
import argparse, math, sys
import numpy as np
import rasterio
from rasterio.warp import transform as warp_transform


def load(tif):
    with rasterio.open(tif) as ds:
        z = ds.read(1).astype("float64")
        nd = ds.nodata
        if nd is not None:
            z = np.where(z == nd, np.nan, z)
        z = np.where(np.isfinite(z), z, np.nan)
        return z, ds.transform, ds.crs


def sample(z, gt, crs, lat, lon, win_m=120.0):
    inv = ~gt
    xs, ys = warp_transform("EPSG:4326", crs, [lon], [lat])
    col, row = inv * (xs[0], ys[0])
    col, row = int(round(col)), int(round(row))
    H, W = z.shape
    if not (0 <= row < H and 0 <= col < W):
        return None
    cell = abs(gt.a)
    k = max(1, int(win_m / cell / 2))
    sub = z[max(0, row-k):row+k+1, max(0, col-k):col+k+1]
    v = sub[np.isfinite(sub)]
    if v.size == 0:
        return None
    floor = float(np.percentile(v, 15))
    top = float(np.nanmax(v))
    med = float(np.median(v))
    return dict(lat=lat, lon=lon, floor_p15=floor, local_max=top, median=med,
                relief=top-floor, n=int(v.size))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tif")
    ap.add_argument("--win-m", type=float, default=120.0)
    ap.add_argument("--pt", action="append", default=[], help="LAT,LON,LABEL")
    ap.add_argument("--transect", default=None,
                    help="LAT1,LON1,LAT2,LON2,N : sample N points along a line")
    args = ap.parse_args()

    z, gt, crs = load(args.tif)
    print("# floor depths (m, negative = below datum); window %.0f m" % args.win_m)

    for spec in args.pt:
        p = spec.split(","); lat, lon = float(p[0]), float(p[1])
        lab = p[2] if len(p) > 2 else ""
        s = sample(z, gt, crs, lat, lon, args.win_m)
        if s is None:
            print("  %-22s OUT OF FRAME / no data" % lab); continue
        print("  %-22s floor=%.2f  top=%.2f  relief=%.2f  (%.0f ft top above floor)"
              % (lab, s["floor_p15"], s["local_max"], s["relief"], s["relief"]*3.28084))

    if args.transect:
        p = args.transect.split(",")
        la1, lo1, la2, lo2, n = float(p[0]), float(p[1]), float(p[2]), float(p[3]), int(p[4])
        print("\n# transect %s->%s, %d pts (down-channel)" %
              ((la1, lo1), (la2, lo2), n))
        print("  idx   lat       lon        floor    top     relief")
        floors = []
        for i in range(n):
            f = i/(n-1)
            la = la1 + (la2-la1)*f; lo = lo1 + (lo2-lo1)*f
            s = sample(z, gt, crs, la, lo, args.win_m)
            if s is None:
                print("  %2d   --- no data ---" % i); continue
            floors.append(s["floor_p15"])
            print("  %2d  %.5f  %.5f  %7.2f  %7.2f  %6.2f"
                  % (i, la, lo, s["floor_p15"], s["local_max"], s["relief"]))
        if len(floors) >= 3:
            fa = np.array(floors)
            print("\n  channel-floor trend: %.2f -> %.2f m  (range %.2f m over transect)"
                  % (fa[0], fa[-1], fa.max()-fa.min()))
            # linear fit residual = how 'bumpy' the floor is vs a smooth grade
            x = np.arange(len(fa)); A = np.polyfit(x, fa, 1)
            resid = fa - np.polyval(A, x)
            print("  smooth-grade slope: %.3f m/step   floor roughness (std resid): %.2f m"
                  % (A[0], resid.std()))


if __name__ == "__main__":
    main()
