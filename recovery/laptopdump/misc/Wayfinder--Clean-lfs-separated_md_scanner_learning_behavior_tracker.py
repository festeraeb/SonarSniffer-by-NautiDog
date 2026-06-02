"""
Behavior Tracker - Records all user actions to learn from patterns

Tracks:
- Search queries (what they look for, success/failure)
- File accesses (what they open, how often)
- Rename operations (what names users choose)
- Navigation patterns (where they go, time spent searching)
- Organization decisions (folder choices, file movements)

Privacy: All data is local, never transmitted. User can delete anytime.
"""

import json
import os
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional, Any
from dataclasses import dataclass, asdict, field
from collections import defaultdict
import hashlib


@dataclass
class SearchEvent:
    """Record of a single search attempt"""
    timestamp: str
    query: str
    results_count: int
    clicked_result: Optional[str]  # Which result they clicked (None = abandoned)
    time_to_click_ms: Optional[int]  # How long to find what they wanted
    refined_query: Optional[str]  # Did they refine the search?
    session_id: str
    

@dataclass
class FileAccessEvent:
    """Record of file being opened/accessed"""
    timestamp: str
    file_path: str
    file_type: str
    access_type: str  # 'open', 'preview', 'edit', 'rename', 'move', 'delete'
    previous_name: Optional[str]  # For renames
    new_name: Optional[str]  # For renames
    source_path: Optional[str]  # For moves
    dest_path: Optional[str]  # For moves
    session_id: str


@dataclass 
class NavigationEvent:
    """Record of folder navigation and browsing"""
    timestamp: str
    path: str
    time_spent_seconds: float
    files_viewed: int
    action_taken: str  # 'opened_file', 'searched', 'navigated_away', 'created_file'
    session_id: str


@dataclass
class OrganizationDecision:
    """Record of user's organizational choices"""
    timestamp: str
    decision_type: str  # 'approve_suggestion', 'reject_suggestion', 'custom_name', 'folder_choice'
    suggested_value: Optional[str]
    user_value: str
    file_path: str
    context: Dict[str, Any]  # Additional info about the decision


@dataclass
class UserSession:
    """A session of user activity"""
    session_id: str
    start_time: str
    end_time: Optional[str] = None
    searches: List[SearchEvent] = field(default_factory=list)
    file_accesses: List[FileAccessEvent] = field(default_factory=list)
    navigation: List[NavigationEvent] = field(default_factory=list)
    decisions: List[OrganizationDecision] = field(default_factory=list)


class BehaviorTracker:
    """
    Main behavior tracking engine.
    Stores all events locally in JSON for analysis.
    """
    
    def __init__(self, data_dir: str = None):
        self.data_dir = Path(data_dir or os.path.expanduser("~/.wayfinder/learning"))
        self.data_dir.mkdir(parents=True, exist_ok=True)
        
        self.events_file = self.data_dir / "behavior_events.json"
        self.sessions_file = self.data_dir / "sessions.json"
        self.stats_file = self.data_dir / "user_stats.json"
        
        self.current_session: Optional[UserSession] = None
        self._load_data()
    
    def _load_data(self):
        """Load existing tracking data"""
        self.events = self._load_json(self.events_file, default={
            'searches': [],
            'file_accesses': [],
            'navigation': [],
            'decisions': []
        })
        self.sessions = self._load_json(self.sessions_file, default=[])
        self.stats = self._load_json(self.stats_file, default={
            'total_searches': 0,
            'successful_searches': 0,
            'total_files_accessed': 0,
            'renames_performed': 0,
            'suggestions_accepted': 0,
            'suggestions_rejected': 0,
            'suggestions_customized': 0,
            'first_seen': None,
            'last_seen': None,
            'skill_scores': {}
        })
    
    def _load_json(self, path: Path, default: Any) -> Any:
        """Load JSON file or return default"""
        if path.exists():
            try:
                with open(path, 'r', encoding='utf-8') as f:
                    return json.load(f)
            except:
                return default
        return default
    
    def _save_data(self):
        """Persist all tracking data"""
        with open(self.events_file, 'w', encoding='utf-8') as f:
            json.dump(self.events, f, indent=2, default=str)
        with open(self.sessions_file, 'w', encoding='utf-8') as f:
            json.dump(self.sessions, f, indent=2, default=str)
        with open(self.stats_file, 'w', encoding='utf-8') as f:
            json.dump(self.stats, f, indent=2, default=str)
    
    def _generate_session_id(self) -> str:
        """Generate unique session ID"""
        return hashlib.md5(
            f"{datetime.now().isoformat()}-{os.getpid()}".encode()
        ).hexdigest()[:12]
    
    # === Session Management ===
    
    def start_session(self) -> str:
        """Start a new tracking session"""
        session_id = self._generate_session_id()
        self.current_session = UserSession(
            session_id=session_id,
            start_time=datetime.now().isoformat()
        )
        
        # Update stats
        now = datetime.now().isoformat()
        if not self.stats['first_seen']:
            self.stats['first_seen'] = now
        self.stats['last_seen'] = now
        
        return session_id
    
    def end_session(self):
        """End current session and save"""
        if self.current_session:
            self.current_session.end_time = datetime.now().isoformat()
            self.sessions.append(asdict(self.current_session))
            self.current_session = None
            self._save_data()
    
    # === Event Recording ===
    
    def record_search(
        self,
        query: str,
        results_count: int,
        clicked_result: Optional[str] = None,
        time_to_click_ms: Optional[int] = None,
        refined_query: Optional[str] = None
    ):
        """Record a search event"""
        if not self.current_session:
            self.start_session()
        
        event = SearchEvent(
            timestamp=datetime.now().isoformat(),
            query=query,
            results_count=results_count,
            clicked_result=clicked_result,
            time_to_click_ms=time_to_click_ms,
            refined_query=refined_query,
            session_id=self.current_session.session_id
        )
        
        self.current_session.searches.append(event)
        self.events['searches'].append(asdict(event))
        
        # Update stats
        self.stats['total_searches'] += 1
        if clicked_result:
            self.stats['successful_searches'] += 1
        
        self._save_data()
    
    def record_file_access(
        self,
        file_path: str,
        access_type: str,
        previous_name: Optional[str] = None,
        new_name: Optional[str] = None,
        source_path: Optional[str] = None,
        dest_path: Optional[str] = None
    ):
        """Record file access event"""
        if not self.current_session:
            self.start_session()
        
        file_type = Path(file_path).suffix.lower() if file_path else 'unknown'
        
        event = FileAccessEvent(
            timestamp=datetime.now().isoformat(),
            file_path=file_path,
            file_type=file_type,
            access_type=access_type,
            previous_name=previous_name,
            new_name=new_name,
            source_path=source_path,
            dest_path=dest_path,
            session_id=self.current_session.session_id
        )
        
        self.current_session.file_accesses.append(event)
        self.events['file_accesses'].append(asdict(event))
        
        # Update stats
        self.stats['total_files_accessed'] += 1
        if access_type == 'rename':
            self.stats['renames_performed'] += 1
        
        self._save_data()
    
    def record_navigation(
        self,
        path: str,
        time_spent_seconds: float,
        files_viewed: int,
        action_taken: str
    ):
        """Record folder navigation"""
        if not self.current_session:
            self.start_session()
        
        event = NavigationEvent(
            timestamp=datetime.now().isoformat(),
            path=path,
            time_spent_seconds=time_spent_seconds,
            files_viewed=files_viewed,
            action_taken=action_taken,
            session_id=self.current_session.session_id
        )
        
        self.current_session.navigation.append(event)
        self.events['navigation'].append(asdict(event))
        self._save_data()
    
    def record_decision(
        self,
        decision_type: str,
        user_value: str,
        file_path: str,
        suggested_value: Optional[str] = None,
        context: Optional[Dict] = None
    ):
        """Record organizational decision"""
        if not self.current_session:
            self.start_session()
        
        decision = OrganizationDecision(
            timestamp=datetime.now().isoformat(),
            decision_type=decision_type,
            suggested_value=suggested_value,
            user_value=user_value,
            file_path=file_path,
            context=context or {}
        )
        
        self.current_session.decisions.append(decision)
        self.events['decisions'].append(asdict(decision))
        
        # Update suggestion stats
        if decision_type == 'approve_suggestion':
            self.stats['suggestions_accepted'] += 1
        elif decision_type == 'reject_suggestion':
            self.stats['suggestions_rejected'] += 1
        elif decision_type == 'custom_name':
            self.stats['suggestions_customized'] += 1
        
        self._save_data()
    
    # === Analytics ===
    
    def get_search_patterns(self, days: int = 30) -> Dict:
        """Analyze search patterns for the last N days"""
        cutoff = datetime.now() - timedelta(days=days)
        recent_searches = [
            s for s in self.events['searches']
            if datetime.fromisoformat(s['timestamp']) > cutoff
        ]
        
        # Find common query patterns
        query_words = defaultdict(int)
        failed_queries = []
        refined_searches = []
        
        for search in recent_searches:
            # Count word frequencies
            for word in search['query'].lower().split():
                query_words[word] += 1
            
            # Track failures (no click)
            if not search.get('clicked_result'):
                failed_queries.append(search['query'])
            
            # Track refinements (struggle indicator)
            if search.get('refined_query'):
                refined_searches.append({
                    'original': search['query'],
                    'refined': search['refined_query']
                })
        
        return {
            'total_searches': len(recent_searches),
            'common_terms': dict(sorted(query_words.items(), key=lambda x: -x[1])[:20]),
            'failed_queries': failed_queries[-10:],  # Last 10 failures
            'search_refinements': refined_searches[-10:],
            'success_rate': self._calculate_success_rate(recent_searches)
        }
    
    def get_file_patterns(self, days: int = 30) -> Dict:
        """Analyze file access patterns"""
        cutoff = datetime.now() - timedelta(days=days)
        recent_accesses = [
            a for a in self.events['file_accesses']
            if datetime.fromisoformat(a['timestamp']) > cutoff
        ]
        
        # Count by type
        type_counts = defaultdict(int)
        access_counts = defaultdict(int)
        renames = []
        
        for access in recent_accesses:
            type_counts[access['file_type']] += 1
            access_counts[access['access_type']] += 1
            
            if access['access_type'] == 'rename':
                renames.append({
                    'from': access['previous_name'],
                    'to': access['new_name'],
                    'file': access['file_path']
                })
        
        return {
            'total_accesses': len(recent_accesses),
            'by_file_type': dict(type_counts),
            'by_access_type': dict(access_counts),
            'recent_renames': renames[-10:]
        }
    
    def get_naming_preferences(self) -> Dict:
        """Learn user's preferred naming patterns from their renames"""
        renames = [
            a for a in self.events['file_accesses']
            if a['access_type'] == 'rename' and a.get('new_name')
        ]
        
        # Analyze naming patterns
        patterns = {
            'uses_dates': 0,
            'uses_underscores': 0,
            'uses_hyphens': 0,
            'uses_camelCase': 0,
            'uses_lowercase': 0,
            'uses_prefixes': [],
            'average_length': 0,
        }
        
        if not renames:
            return {'learned_patterns': None, 'sample_size': 0}
        
        lengths = []
        prefixes = defaultdict(int)
        
        for rename in renames:
            name = rename['new_name']
            
            # Date patterns (YYYY, YYYYMM, YYYYMMDD)
            if any(c.isdigit() for c in name) and len([c for c in name if c.isdigit()]) >= 4:
                patterns['uses_dates'] += 1
            
            # Separators
            if '_' in name:
                patterns['uses_underscores'] += 1
            if '-' in name:
                patterns['uses_hyphens'] += 1
            
            # Case patterns
            if name == name.lower():
                patterns['uses_lowercase'] += 1
            if any(c.isupper() for c in name[1:]):  # camelCase check
                patterns['uses_camelCase'] += 1
            
            # Prefix detection (first word before separator)
            for sep in ['_', '-', ' ']:
                if sep in name:
                    prefix = name.split(sep)[0]
                    if len(prefix) < 15:  # Reasonable prefix length
                        prefixes[prefix] += 1
                    break
            
            lengths.append(len(name))
        
        patterns['average_length'] = sum(lengths) / len(lengths) if lengths else 0
        patterns['uses_prefixes'] = [
            {'prefix': k, 'count': v} 
            for k, v in sorted(prefixes.items(), key=lambda x: -x[1])[:5]
        ]
        
        # Determine primary style
        total = len(renames)
        patterns['primary_separator'] = 'underscore' if patterns['uses_underscores'] > patterns['uses_hyphens'] else 'hyphen'
        patterns['date_frequency'] = patterns['uses_dates'] / total if total else 0
        
        return {
            'learned_patterns': patterns,
            'sample_size': len(renames)
        }
    
    def get_suggestion_effectiveness(self) -> Dict:
        """How well are suggestions being received?"""
        accepted = self.stats.get('suggestions_accepted', 0)
        rejected = self.stats.get('suggestions_rejected', 0)
        customized = self.stats.get('suggestions_customized', 0)
        
        total = accepted + rejected + customized
        
        if total == 0:
            return {
                'total_suggestions': 0,
                'acceptance_rate': 0,
                'rejection_rate': 0,
                'customization_rate': 0,
                'effective': None  # Not enough data
            }
        
        return {
            'total_suggestions': total,
            'acceptance_rate': accepted / total,
            'rejection_rate': rejected / total,
            'customization_rate': customized / total,
            'effective': (accepted + customized) / total > 0.5
        }
    
    def _calculate_success_rate(self, searches: List[Dict]) -> float:
        """Calculate search success rate"""
        if not searches:
            return 0.0
        successful = sum(1 for s in searches if s.get('clicked_result'))
        return successful / len(searches)
    
    # === Data Management ===
    
    def clear_all_data(self):
        """Clear all tracking data (privacy feature)"""
        self.events = {'searches': [], 'file_accesses': [], 'navigation': [], 'decisions': []}
        self.sessions = []
        self.stats = {
            'total_searches': 0,
            'successful_searches': 0,
            'total_files_accessed': 0,
            'renames_performed': 0,
            'suggestions_accepted': 0,
            'suggestions_rejected': 0,
            'suggestions_customized': 0,
            'first_seen': None,
            'last_seen': None,
            'skill_scores': {}
        }
        self._save_data()
    
    def export_data(self) -> Dict:
        """Export all data for backup"""
        return {
            'events': self.events,
            'sessions': self.sessions,
            'stats': self.stats,
            'exported_at': datetime.now().isoformat()
        }
    
    def get_summary(self) -> Dict:
        """Get summary of tracked behavior"""
        return {
            'total_searches': self.stats.get('total_searches', 0),
            'search_success_rate': (
                self.stats.get('successful_searches', 0) / 
                max(self.stats.get('total_searches', 1), 1)
            ),
            'files_accessed': self.stats.get('total_files_accessed', 0),
            'renames': self.stats.get('renames_performed', 0),
            'suggestion_acceptance': self.get_suggestion_effectiveness(),
            'naming_preferences': self.get_naming_preferences(),
            'first_seen': self.stats.get('first_seen'),
            'last_seen': self.stats.get('last_seen'),
            'days_active': self._calculate_days_active()
        }
    
    def _calculate_days_active(self) -> int:
        """Calculate number of days user has been active"""
        first = self.stats.get('first_seen')
        last = self.stats.get('last_seen')
        if not first or not last:
            return 0
        first_dt = datetime.fromisoformat(first)
        last_dt = datetime.fromisoformat(last)
        return (last_dt - first_dt).days + 1
