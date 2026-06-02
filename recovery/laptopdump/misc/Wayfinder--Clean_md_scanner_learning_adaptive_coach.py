"""
Adaptive Coach - Orchestrates the training wheels system

The coach coordinates between:
- BehaviorTracker: Records what the user does
- DifficultyDetector: Identifies what they struggle with
- SuggestionEngine: Generates helpful suggestions

Key responsibilities:
1. Adjust suggestion intensity based on skill levels
2. Fade out help as user masters skills
3. Re-engage when regression is detected
4. Provide unified API for Tauri/UI layer
"""

from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional, Any, Tuple
from dataclasses import dataclass, asdict, field
import json
import os

from .behavior_tracker import BehaviorTracker
from .difficulty_detector import DifficultyDetector, DifficultyReport, SkillAssessment
from .suggestion_engine import SuggestionEngine, NamingSuggestion, FolderSuggestion


@dataclass
class CoachingSession:
    """Represents a coaching interaction"""
    session_id: str
    started_at: str
    skill_snapshot: Dict[str, float]
    suggestions_offered: int
    suggestions_accepted: int
    suggestions_dismissed: int


@dataclass
class SuggestionResult:
    """Result of a suggestion request with fade-out applied"""
    suggestion_type: str  # 'naming', 'folder', 'convention', 'none'
    content: Optional[Dict]  # The actual suggestion
    intensity: float  # 0.0 (faded) to 1.0 (full guidance)
    reason: str  # Why this intensity
    skill_level: str  # 'struggling', 'learning', 'proficient', 'mastered'
    should_show: bool  # Whether to actually display to user


@dataclass
class CoachingState:
    """Persistent state of the coaching system"""
    total_suggestions_offered: int = 0
    total_suggestions_accepted: int = 0
    total_suggestions_dismissed: int = 0
    last_assessment: Optional[str] = None
    skill_history: Dict[str, List[Dict]] = field(default_factory=dict)
    fade_out_overrides: Dict[str, bool] = field(default_factory=dict)  # User can force-disable per skill


class AdaptiveCoach:
    """
    The main orchestrator for the adaptive learning system.
    
    Usage:
        coach = AdaptiveCoach()
        coach.start_session()
        
        # When user saves a file
        result = coach.get_naming_suggestion(filename, content)
        if result.should_show:
            # Display suggestion to user
            ...
        
        # When user accepts/rejects
        coach.record_suggestion_response(result, accepted=True)
        
        # Get current coaching status
        status = coach.get_status()
    """
    
    # Intensity thresholds
    INTENSITY_FULL = 1.0
    INTENSITY_REGULAR = 0.7
    INTENSITY_OCCASIONAL = 0.3
    INTENSITY_MINIMAL = 0.1
    INTENSITY_NONE = 0.0
    
    # Skill level boundaries
    SKILL_STRUGGLING = 0.4
    SKILL_LEARNING = 0.7
    SKILL_PROFICIENT = 0.85
    
    # How often to show suggestions at each intensity
    SHOW_PROBABILITY = {
        'full': 1.0,       # Every time
        'regular': 0.7,    # 70% of the time  
        'occasional': 0.3, # 30% of the time
        'minimal': 0.1,    # 10% of the time
        'none': 0.0,       # Never
    }
    
    def __init__(self, data_dir: Optional[str] = None):
        """
        Initialize the adaptive coach.
        
        Args:
            data_dir: Directory for storing coaching data. 
                     Defaults to ~/.wayfinder/learning/
        """
        self.data_dir = Path(data_dir) if data_dir else Path.home() / '.wayfinder' / 'learning'
        self.data_dir.mkdir(parents=True, exist_ok=True)
        
        # Initialize components
        self.tracker = BehaviorTracker(str(self.data_dir))
        self.detector = DifficultyDetector(self.tracker)
        self.suggester = SuggestionEngine(self.tracker)
        
        # Load persistent state
        self.state = self._load_state()
        
        # Current session
        self.current_session: Optional[CoachingSession] = None
        
        # Cache for skill assessment (avoid re-computing constantly)
        self._cached_report: Optional[DifficultyReport] = None
        self._cache_time: Optional[datetime] = None
        self._cache_duration = timedelta(minutes=5)
    
    # =========================================================================
    # Session Management
    # =========================================================================
    
    def start_session(self) -> str:
        """Start a new coaching session. Returns session ID."""
        session_id = self.tracker.start_session()
        
        # Get current skill snapshot
        report = self._get_difficulty_report()
        skill_snapshot = {
            name: assessment.score 
            for name, assessment in report.skills.items()
        }
        
        self.current_session = CoachingSession(
            session_id=session_id,
            started_at=datetime.now().isoformat(),
            skill_snapshot=skill_snapshot,
            suggestions_offered=0,
            suggestions_accepted=0,
            suggestions_dismissed=0
        )
        
        return session_id
    
    def end_session(self) -> Dict:
        """End the current session and return summary."""
        if not self.current_session:
            return {'error': 'No active session'}
        
        summary = {
            'session_id': self.current_session.session_id,
            'duration': self._calculate_duration(self.current_session.started_at),
            'suggestions_offered': self.current_session.suggestions_offered,
            'suggestions_accepted': self.current_session.suggestions_accepted,
            'acceptance_rate': self._safe_divide(
                self.current_session.suggestions_accepted,
                self.current_session.suggestions_offered
            ),
            'skill_snapshot': self.current_session.skill_snapshot,
        }
        
        self.tracker.end_session()
        self.current_session = None
        
        return summary
    
    # =========================================================================
    # Suggestion API with Fade-Out Logic
    # =========================================================================
    
    def get_naming_suggestion(
        self, 
        filename: str, 
        content: Optional[str] = None,
        file_path: Optional[str] = None
    ) -> SuggestionResult:
        """
        Get a naming suggestion with fade-out logic applied.
        
        Args:
            filename: Current filename
            content: File content (optional, for smarter suggestions)
            file_path: Full file path (optional)
        
        Returns:
            SuggestionResult with should_show indicating whether to display
        """
        # Get skill level for naming
        report = self._get_difficulty_report()
        naming_skill = report.skills.get('naming_consistency')
        
        if not naming_skill:
            # No data yet, show full guidance
            return self._create_naming_result(
                filename, content, file_path,
                intensity=self.INTENSITY_FULL,
                skill_level='new',
                reason='No naming history - providing full guidance'
            )
        
        # Calculate intensity based on skill
        intensity, skill_level = self._calculate_intensity(naming_skill)
        
        # Check for user override
        if self.state.fade_out_overrides.get('naming_consistency', False):
            return SuggestionResult(
                suggestion_type='none',
                content=None,
                intensity=0.0,
                reason='User disabled naming suggestions',
                skill_level=skill_level,
                should_show=False
            )
        
        # Determine if we should show based on intensity
        should_show = self._should_show_at_intensity(intensity)
        
        if not should_show:
            return SuggestionResult(
                suggestion_type='naming',
                content=None,
                intensity=intensity,
                reason=f'Faded out - skill level: {skill_level}',
                skill_level=skill_level,
                should_show=False
            )
        
        return self._create_naming_result(
            filename, content, file_path,
            intensity=intensity,
            skill_level=skill_level,
            reason=f'Skill: {naming_skill.score:.0%}, Trend: {naming_skill.trend}'
        )
    
    def get_folder_suggestion(
        self,
        file_path: str,
        available_folders: Optional[List[str]] = None
    ) -> SuggestionResult:
        """
        Get a folder suggestion with fade-out logic applied.
        
        Args:
            file_path: Path of file to organize
            available_folders: List of existing folders to consider
        
        Returns:
            SuggestionResult with should_show indicating whether to display
        """
        report = self._get_difficulty_report()
        folder_skill = report.skills.get('folder_organization')
        
        if not folder_skill:
            return self._create_folder_result(
                file_path, available_folders,
                intensity=self.INTENSITY_FULL,
                skill_level='new',
                reason='No folder organization history'
            )
        
        intensity, skill_level = self._calculate_intensity(folder_skill)
        
        if self.state.fade_out_overrides.get('folder_organization', False):
            return SuggestionResult(
                suggestion_type='none',
                content=None,
                intensity=0.0,
                reason='User disabled folder suggestions',
                skill_level=skill_level,
                should_show=False
            )
        
        should_show = self._should_show_at_intensity(intensity)
        
        if not should_show:
            return SuggestionResult(
                suggestion_type='folder',
                content=None,
                intensity=intensity,
                reason=f'Faded out - skill level: {skill_level}',
                skill_level=skill_level,
                should_show=False
            )
        
        return self._create_folder_result(
            file_path, available_folders,
            intensity=intensity,
            skill_level=skill_level,
            reason=f'Skill: {folder_skill.score:.0%}, Trend: {folder_skill.trend}'
        )
    
    def get_search_tip(self, query: str, results_count: int) -> Optional[Dict]:
        """
        Get a search tip if user is struggling with search.
        
        Args:
            query: The search query used
            results_count: Number of results returned
        
        Returns:
            Dict with tip content, or None if skills are good
        """
        report = self._get_difficulty_report()
        search_skill = report.skills.get('search_ability')
        
        if not search_skill:
            return None  # Let them try first
        
        intensity, skill_level = self._calculate_intensity(search_skill)
        
        # Only show tips for struggling/learning users
        if intensity < self.INTENSITY_OCCASIONAL:
            return None
        
        # Generate contextual tips
        if results_count == 0:
            return {
                'type': 'no_results',
                'tip': 'Try using different keywords or check spelling',
                'examples': ['Use broader terms', 'Try related concepts'],
                'intensity': intensity
            }
        elif search_skill.score < self.SKILL_LEARNING:
            return {
                'type': 'general',
                'tip': 'Describe what you\'re looking for naturally',
                'examples': ['meeting notes about budget', 'that python script for parsing'],
                'intensity': intensity
            }
        
        return None
    
    # =========================================================================
    # Response Recording
    # =========================================================================
    
    def record_suggestion_response(
        self,
        suggestion_type: str,
        accepted: bool,
        original_value: str,
        suggested_value: str,
        final_value: str
    ):
        """
        Record user's response to a suggestion.
        
        Args:
            suggestion_type: 'naming', 'folder', 'convention'
            accepted: Whether user accepted the suggestion
            original_value: Original filename/path
            suggested_value: What we suggested
            final_value: What user actually used
        """
        # Update session counts
        if self.current_session:
            if accepted:
                self.current_session.suggestions_accepted += 1
            else:
                self.current_session.suggestions_dismissed += 1
        
        # Update global state
        if accepted:
            self.state.total_suggestions_accepted += 1
        else:
            self.state.total_suggestions_dismissed += 1
        
        self._save_state()
        
        # Record in behavior tracker
        self.tracker.record_organization_decision(
            file_path=original_value,
            suggested_name=suggested_value,
            chosen_name=final_value,
            suggestion_source='coach',
            accepted=accepted
        )
        
        # Invalidate cache to get fresh assessment
        self._cached_report = None
    
    # =========================================================================
    # Status and Reporting
    # =========================================================================
    
    def get_status(self) -> Dict:
        """
        Get current coaching status - useful for UI display.
        
        Returns:
            Dict with skill levels, intensities, and recommendations
        """
        report = self._get_difficulty_report()
        
        skills_status = {}
        for name, assessment in report.skills.items():
            intensity, level = self._calculate_intensity(assessment)
            skills_status[name] = {
                'score': assessment.score,
                'level': level,
                'trend': assessment.trend,
                'intensity': intensity,
                'guidance_active': intensity > self.INTENSITY_MINIMAL,
                'samples': assessment.samples_count,
                'confidence': assessment.confidence
            }
        
        return {
            'overall_skill': report.overall_skill,
            'skills': skills_status,
            'suggestions': {
                'offered': self.state.total_suggestions_offered,
                'accepted': self.state.total_suggestions_accepted,
                'dismissed': self.state.total_suggestions_dismissed,
                'acceptance_rate': self._safe_divide(
                    self.state.total_suggestions_accepted,
                    self.state.total_suggestions_offered
                )
            },
            'recommendations': report.recommendations,
            'struggles': [s['area'] for s in report.struggles] if report.struggles else [],
            'improvements': [i['area'] for i in report.improvements] if report.improvements else [],
            'regressions': [r['area'] for r in report.regressions] if report.regressions else [],
            'session_active': self.current_session is not None
        }
    
    def get_skill_history(self, skill_name: str, days: int = 30) -> List[Dict]:
        """Get historical skill scores for visualization."""
        history = self.state.skill_history.get(skill_name, [])
        cutoff = datetime.now() - timedelta(days=days)
        
        return [
            entry for entry in history
            if datetime.fromisoformat(entry['date']) > cutoff
        ]
    
    def get_fade_status(self) -> Dict[str, Dict]:
        """
        Get fade-out status for each skill area.
        
        Returns which skills are still getting suggestions and which have faded.
        """
        report = self._get_difficulty_report()
        
        fade_status = {}
        for name, assessment in report.skills.items():
            intensity, level = self._calculate_intensity(assessment)
            
            fade_status[name] = {
                'level': level,
                'intensity': intensity,
                'faded_out': intensity <= self.INTENSITY_MINIMAL,
                'override_disabled': self.state.fade_out_overrides.get(name, False),
                'will_re_engage': assessment.trend == 'regressing'
            }
        
        return fade_status
    
    # =========================================================================
    # User Controls
    # =========================================================================
    
    def set_fade_out_override(self, skill_name: str, disabled: bool):
        """
        Allow user to manually disable suggestions for a skill.
        
        Args:
            skill_name: 'naming_consistency', 'folder_organization', etc.
            disabled: True to disable suggestions, False to re-enable
        """
        self.state.fade_out_overrides[skill_name] = disabled
        self._save_state()
    
    def reset_skill_tracking(self, skill_name: Optional[str] = None):
        """
        Reset skill tracking to start fresh.
        
        Args:
            skill_name: Specific skill to reset, or None for all
        """
        if skill_name:
            self.state.skill_history[skill_name] = []
        else:
            self.state.skill_history = {}
        
        self._save_state()
        self._cached_report = None
    
    def force_re_engagement(self, skill_name: str):
        """
        Force re-engagement of suggestions for a skill.
        
        Useful when user knows they need help again.
        """
        # Remove any fade-out override
        if skill_name in self.state.fade_out_overrides:
            del self.state.fade_out_overrides[skill_name]
        
        # Clear history to reset to "learning" state
        self.state.skill_history[skill_name] = []
        
        self._save_state()
        self._cached_report = None
    
    # =========================================================================
    # Internal: Intensity Calculation
    # =========================================================================
    
    def _calculate_intensity(self, assessment: SkillAssessment) -> Tuple[float, str]:
        """
        Calculate suggestion intensity based on skill assessment.
        
        Returns:
            Tuple of (intensity_value, skill_level_name)
        """
        score = assessment.score
        trend = assessment.trend
        
        # Base intensity on score
        if score < self.SKILL_STRUGGLING:
            base_intensity = self.INTENSITY_FULL
            level = 'struggling'
        elif score < self.SKILL_LEARNING:
            base_intensity = self.INTENSITY_REGULAR
            level = 'learning'
        elif score < self.SKILL_PROFICIENT:
            base_intensity = self.INTENSITY_OCCASIONAL
            level = 'proficient'
        else:
            base_intensity = self.INTENSITY_MINIMAL
            level = 'mastered'
        
        # Adjust for trend
        if trend == 'regressing':
            # Increase intensity if skill is declining
            adjusted = min(base_intensity + 0.2, self.INTENSITY_FULL)
            if level == 'mastered':
                level = 'proficient'  # Downgrade status
        elif trend == 'improving':
            # Decrease intensity faster if improving
            adjusted = max(base_intensity - 0.1, self.INTENSITY_NONE)
        else:
            adjusted = base_intensity
        
        # Low confidence means more help
        if assessment.confidence < 0.5:
            adjusted = max(adjusted, self.INTENSITY_REGULAR)
        
        return adjusted, level
    
    def _should_show_at_intensity(self, intensity: float) -> bool:
        """
        Probabilistically decide whether to show suggestion at given intensity.
        
        This creates natural fade-out effect rather than hard cutoffs.
        """
        import random
        return random.random() < intensity
    
    # =========================================================================
    # Internal: Suggestion Creation
    # =========================================================================
    
    def _create_naming_result(
        self,
        filename: str,
        content: Optional[str],
        file_path: Optional[str],
        intensity: float,
        skill_level: str,
        reason: str
    ) -> SuggestionResult:
        """Create a naming suggestion result."""
        # Get suggestion from engine
        suggestion = self.suggester.suggest_filename(filename, content)
        
        if self.current_session:
            self.current_session.suggestions_offered += 1
        self.state.total_suggestions_offered += 1
        
        return SuggestionResult(
            suggestion_type='naming',
            content=asdict(suggestion) if suggestion else None,
            intensity=intensity,
            reason=reason,
            skill_level=skill_level,
            should_show=True
        )
    
    def _create_folder_result(
        self,
        file_path: str,
        available_folders: Optional[List[str]],
        intensity: float,
        skill_level: str,
        reason: str
    ) -> SuggestionResult:
        """Create a folder suggestion result."""
        # Get suggestion from engine
        suggestion = self.suggester.suggest_folder(file_path, available_folders)
        
        if self.current_session:
            self.current_session.suggestions_offered += 1
        self.state.total_suggestions_offered += 1
        
        return SuggestionResult(
            suggestion_type='folder',
            content=asdict(suggestion) if suggestion else None,
            intensity=intensity,
            reason=reason,
            skill_level=skill_level,
            should_show=True
        )
    
    # =========================================================================
    # Internal: Difficulty Report Caching
    # =========================================================================
    
    def _get_difficulty_report(self) -> DifficultyReport:
        """Get difficulty report with caching."""
        now = datetime.now()
        
        if (self._cached_report and self._cache_time and 
            now - self._cache_time < self._cache_duration):
            return self._cached_report
        
        self._cached_report = self.detector.assess_all_skills()
        self._cache_time = now
        
        # Update skill history
        for name, assessment in self._cached_report.skills.items():
            if name not in self.state.skill_history:
                self.state.skill_history[name] = []
            
            self.state.skill_history[name].append({
                'date': now.isoformat(),
                'score': assessment.score,
                'trend': assessment.trend
            })
            
            # Keep only last 100 entries
            self.state.skill_history[name] = self.state.skill_history[name][-100:]
        
        self.state.last_assessment = now.isoformat()
        self._save_state()
        
        return self._cached_report
    
    # =========================================================================
    # Internal: State Persistence
    # =========================================================================
    
    def _load_state(self) -> CoachingState:
        """Load persistent state from disk."""
        state_file = self.data_dir / 'coaching_state.json'
        
        if state_file.exists():
            try:
                with open(state_file, 'r') as f:
                    data = json.load(f)
                    return CoachingState(
                        total_suggestions_offered=data.get('total_suggestions_offered', 0),
                        total_suggestions_accepted=data.get('total_suggestions_accepted', 0),
                        total_suggestions_dismissed=data.get('total_suggestions_dismissed', 0),
                        last_assessment=data.get('last_assessment'),
                        skill_history=data.get('skill_history', {}),
                        fade_out_overrides=data.get('fade_out_overrides', {})
                    )
            except (json.JSONDecodeError, KeyError):
                pass
        
        return CoachingState()
    
    def _save_state(self):
        """Save persistent state to disk."""
        state_file = self.data_dir / 'coaching_state.json'
        
        with open(state_file, 'w') as f:
            json.dump({
                'total_suggestions_offered': self.state.total_suggestions_offered,
                'total_suggestions_accepted': self.state.total_suggestions_accepted,
                'total_suggestions_dismissed': self.state.total_suggestions_dismissed,
                'last_assessment': self.state.last_assessment,
                'skill_history': self.state.skill_history,
                'fade_out_overrides': self.state.fade_out_overrides
            }, f, indent=2)
    
    # =========================================================================
    # Internal: Utilities
    # =========================================================================
    
    def _calculate_duration(self, start_time: str) -> str:
        """Calculate human-readable duration from ISO timestamp."""
        start = datetime.fromisoformat(start_time)
        duration = datetime.now() - start
        
        minutes = int(duration.total_seconds() / 60)
        if minutes < 60:
            return f"{minutes}m"
        
        hours = minutes // 60
        mins = minutes % 60
        return f"{hours}h {mins}m"
    
    def _safe_divide(self, a: int, b: int) -> float:
        """Safe division returning 0 if denominator is 0."""
        return a / b if b > 0 else 0.0


# =============================================================================
# JSON-RPC API for Tauri Bridge
# =============================================================================

def create_coach_api() -> Dict[str, callable]:
    """
    Create API functions for the Tauri bridge.
    
    Returns dict of {command_name: handler_function}
    """
    coach = AdaptiveCoach()
    
    return {
        'coach_start_session': lambda: coach.start_session(),
        'coach_end_session': lambda: coach.end_session(),
        'coach_get_naming_suggestion': lambda filename, content=None: 
            asdict(coach.get_naming_suggestion(filename, content)),
        'coach_get_folder_suggestion': lambda file_path, folders=None:
            asdict(coach.get_folder_suggestion(file_path, folders)),
        'coach_get_search_tip': coach.get_search_tip,
        'coach_record_response': coach.record_suggestion_response,
        'coach_get_status': coach.get_status,
        'coach_get_fade_status': coach.get_fade_status,
        'coach_set_override': coach.set_fade_out_override,
        'coach_reset_tracking': coach.reset_skill_tracking,
        'coach_force_reengagement': coach.force_re_engagement,
    }
