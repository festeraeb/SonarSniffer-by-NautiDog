import subprocess
import sys

pkgs = ['pystac-client', 'planetary-computer', 'rasterio']

def run(cmd):
    print('RUN:', ' '.join(cmd))
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    print(p.stdout)
    return p.returncode

def _missing_pkgs(pkg_names):
    """Return only packages not already importable/installed."""
    import importlib.util
    # Normalize: pystac-client → pystac_client, planetary-computer → planetary_computer
    missing = []
    for pkg in pkg_names:
        module_name = pkg.split('[')[0].replace('-', '_')
        if importlib.util.find_spec(module_name) is None:
            missing.append(pkg)
    return missing

if __name__ == '__main__':
    to_install = _missing_pkgs(pkgs)
    if not to_install:
        print('All packages already installed — nothing to do.')
        sys.exit(0)
    print(f'Missing packages: {to_install}')
    rc = run([sys.executable, '-m', 'pip', 'install'] + to_install)
    if rc == 0:
        print('Install complete')
    else:
        print('Install finished with errors; check output')
