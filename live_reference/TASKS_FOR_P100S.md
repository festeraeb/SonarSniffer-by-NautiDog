# Tasks for P100 pair

Use these diffs to compare laptop variants against live and decide merge/reject.

## P100-0 queue

- `pipelines/bag/advanced_bag_scanner_runner.py` <= `live_reference/bag__advanced_bag_scanner_runner.py__SIMILAR_TO_LIVE__laptop-code-pipelines__advanced_bag_scanner_runner.py`
- `pipelines/mag/erie_wellhead_discriminator.py` <= `live_reference/mag__erie_wellhead_discriminator.py__SIMILAR_TO_LIVE__laptop-code-pipelines__erie_wellhead_discriminator.py`
- `repo_root/cmr_search.py` <= `live_reference/repo_root__cmr_search.py__SIMILAR_TO_LIVE__laptopdump-programming-root__cmr_search.py`
- `repo_root/weather_service.py` <= `live_reference/repo_root__weather_service.py__SIMILAR_TO_LIVE__laptopdump-programming-root__weather_service.py`

## P100-1 queue

- `pipelines/mag/adaptive_background_scan.py` <= `live_reference/mag__adaptive_background_scan.py__SIMILAR_TO_LIVE__laptop-code-pipelines__adaptive_background_scan.py`
- `pipelines/mag/mag_data_pipeline.py` <= `live_reference/mag__mag_data_pipeline.py__SIMILAR_TO_LIVE__laptop-code-pipelines__mag_data_pipeline.py`
- `repo_root/universal_downloader.py` <= `live_reference/repo_root__universal_downloader.py__SIMILAR_TO_LIVE__laptopdump-programming-root__universal_downloader.py`

## Diff command template

```bash
diff -u /codebase/projects/${LIVE_KEY#pipelines/} /codebase/repos/wreckhunter2000-1/${REF_PATH}
```
