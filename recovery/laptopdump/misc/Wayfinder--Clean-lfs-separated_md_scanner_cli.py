"""
CLI interface for md-scanner.
"""
import click
from pathlib import Path
from tqdm import tqdm
from md_scanner.scanner import FileScanner
from md_scanner.embeddings import EmbeddingEngine
from md_scanner.clustering import ClusteringEngine
from md_scanner.timeline import TimelineEngine
from md_scanner.search import SearchEngine

INDEX_DIR = Path.home() / '.md_index'

@click.group()
def cli():
    """Markdown Scanner & Organizer - Find your scattered thoughts."""
    pass

@cli.command()
@click.argument('directory', type=click.Path(exists=True))
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
def scan(directory, index_dir):
    """Scan directory for markdown files."""
    click.echo(f"Scanning {directory}...")

    scanner = FileScanner(index_dir)

    def progress_update(count):
        click.echo(f"  Found {count} files...", nl=False)
        click.echo('\r', nl=False)

    files = scanner.scan(directory, progress_callback=progress_update)
    click.echo(f"\nFound {len(files)} markdown files")

    scanner.save_index(files)
    click.echo(f"Index saved to {scanner.index_file}")

@cli.command()
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
def embed(index_dir):
    """Generate embeddings for indexed files."""
    scanner = FileScanner(index_dir)
    files = scanner.load_index()

    if not files:
        click.echo("No index found. Run 'scan' first.")
        return

    click.echo(f"Generating embeddings for {len(files)} files...")

    engine = EmbeddingEngine(index_dir=index_dir)
    file_paths = [f.path for f in files]

    with tqdm(total=len(file_paths)) as pbar:
        def progress_update(current, total):
            pbar.update(1)

        engine.generate_embeddings(file_paths, progress_callback=progress_update)

    click.echo(f"Embeddings saved to {engine.embeddings_file}")

@cli.command()
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
@click.option('--num-clusters', type=int, default=None, help='Number of clusters')
def cluster(index_dir, num_clusters):
    """Cluster files into semantic groups."""
    embedding_engine = EmbeddingEngine(index_dir=index_dir)

    if not embedding_engine.load_embeddings():
        click.echo("No embeddings found. Run 'embed' first.")
        return

    click.echo(f"Clustering {len(embedding_engine.file_paths)} files...")

    clustering_engine = ClusteringEngine(index_dir=index_dir)

    def progress_update(stage, message):
        click.echo(f"  {message}")

    clusters = clustering_engine.cluster(
        embedding_engine.embeddings,
        embedding_engine.file_paths,
        n_clusters=num_clusters,
        progress_callback=progress_update
    )

    click.echo(f"\nCreated {len(clusters)} clusters:")
    for summary in clustering_engine.list_clusters():
        click.echo(
            f"  Cluster {summary['id']}: {summary['file_count']} files - "
            f"{', '.join(summary['sample_files'][:2])}..."
        )

@cli.command()
@click.argument('query')
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
@click.option('--top-k', type=int, default=10, help='Number of results')
@click.option('--semantic-weight', type=float, default=0.7, help='Semantic weight')
def search(query, index_dir, top_k, semantic_weight):
    """Search for files by semantic similarity."""
    embedding_engine = EmbeddingEngine(index_dir=index_dir)

    if not embedding_engine.load_embeddings():
        click.echo("No embeddings found. Run 'embed' first.")
        return

    click.echo(f"Searching for: '{query}'")

    search_engine = SearchEngine(embedding_engine)
    results = search_engine.search(query, semantic_weight=semantic_weight, top_k=top_k)

    click.echo(f"\nFound {len(results)} results:")
    for i, (file_path, score) in enumerate(results, 1):
        file_name = Path(file_path).name
        click.echo(f"  {i}. {file_name} ({score:.3f})")
        click.echo(f"     => {file_path}")

@cli.command()
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
def list_clusters(index_dir):
    """List all clusters."""
    clustering_engine = ClusteringEngine(index_dir=index_dir)

    if not clustering_engine.load_clusters():
        click.echo("No clusters found. Run 'cluster' first.")
        return

    clusters = clustering_engine.list_clusters()

    click.echo(f"Total clusters: {len(clusters)}\n")
    for summary in clusters:
        click.echo(f"Cluster {summary['id']} ({summary['file_count']} files):")
        for file_name in summary['sample_files']:
            click.echo(f"  - {file_name}")
        click.echo()

@cli.command()
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
@click.option('--days', type=int, default=30, help='Days to show')
def timeline(index_dir, days):
    """Show recent files organized by date."""
    scanner = FileScanner(index_dir)
    files = scanner.load_index()

    if not files:
        click.echo("No index found. Run 'scan' first.")
        return

    timeline_engine = TimelineEngine()
    timeline_engine.set_data(files)

    timeline_data = timeline_engine.get_timeline_by_date(days=days)

    click.echo(f"Files modified in last {days} days:\n")
    for date_str, file_list in timeline_data.items():
        click.echo(f"{date_str} ({len(file_list)} files):")
        for file_path in file_list[:5]:
            file_name = Path(file_path).name
            click.echo(f"  - {file_name}")
        if len(file_list) > 5:
            click.echo(f"  ... and {len(file_list) - 5} more")
        click.echo()

@cli.command()
@click.option('--index-dir', default=str(INDEX_DIR), help='Index directory')
def stats(index_dir):
    """Show statistics about indexed files."""
    scanner = FileScanner(index_dir)
    files = scanner.load_index()

    if not files:
        click.echo("No index found. Run 'scan' first.")
        return

    timeline_engine = TimelineEngine()
    timeline_engine.set_data(files)

    click.echo("Index Statistics:")
    click.echo(f"  Total files: {len(files)}")

    # Time bucket stats
    buckets = timeline_engine.get_bucket_summary()
    click.echo("\n  Files by age:")
    for bucket_name, count in buckets.items():
        if count > 0:
            click.echo(f"    {bucket_name}: {count}")

    # Size stats
    total_size = sum(f.size for f in files)
    click.echo(f"\n  Total size: {total_size / 1024 / 1024:.2f} MB")

    # Clustering info
    clustering_engine = ClusteringEngine(index_dir=index_dir)
    if clustering_engine.load_clusters():
        click.echo(f"  Clusters: {len(clustering_engine.clusters)}")

if __name__ == '__main__':
    cli()
