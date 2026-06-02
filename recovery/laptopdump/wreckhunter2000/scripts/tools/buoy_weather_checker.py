import pandas as pd

try:
    from seebuoy import NDBC
except ImportError:
    NDBC = None
    print('WARNING: seebuoy not installed. buoy_weather_checker will run in degraded mode.')


def is_ideal_glint_day(station_id: str, date: pd.Timestamp):
    ndbc = NDBC()
    if NDBC is None:
        return False

    df = ndbc.get_station(station_id)
    df.index = pd.to_datetime(df.index)

    if date not in df.index:
        return False

    daily = df.loc[date]
    if 'WSPD' not in daily or 'WVHT' not in daily:
        return False

    avg = daily[['WSPD', 'WVHT']].mean()
    return avg['WSPD'] < 5.0 and avg['WVHT'] < 0.5  # Stricter: calm conditions for glint


def select_glint_windows(station_id: str, year: int, max_days: int = 8):
    if NDBC is None:
        return []

    ndbc = NDBC()
    df = ndbc.get_station(station_id)
    df.index = pd.to_datetime(df.index)

    start = pd.Timestamp(f'{year}-01-01')
    end = pd.Timestamp(f'{year}-12-31')

    valid_days = []
    for date, group in df.loc[start:end].groupby(df.index.date):
        daily = group[['WSPD', 'WVHT']].mean()
        if daily['WSPD'] < 5.0 and daily['WVHT'] < 0.5:  # Stricter: calm conditions for glint
            valid_days.append(pd.Timestamp(date))
        if len(valid_days) >= max_days:
            break

    return valid_days
