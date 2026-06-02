"""
Difficulty Detector - Identifies user struggles and skill gaps

Detects:
- Repeated failed searches (can't find what they're looking for)
- Search refinements (had to rephrase to find results)
- Long navigation times (wandering through folders)
- Frequent renames (files poorly named initially)
- Poor file organization (deep nesting, scattered files)
- Naming inconsistencies (mixing conventions)

Outputs skill scores that drive suggestions intensity.
"""

from datetime import datetime, timedelta
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass
from collections import defaultdict
import re
from pathlib import Path

from .behavior_tracker import BehaviorTracker


@dataclass
class SkillAssessment:
    """Assessment of user's skill in a specific area"""

    skill_name: str
    score: float  # 0.0 (struggling) to 1.0 (mastered)
    confidence: float  # How confident we are in this score
    trend: str  # 'improving', 'stable', 'regressing', 'new'
    evidence: List[str]  # What led to this assessment
    last_updated: str
    samples_count: int


@dataclass
class DifficultyReport:
    """Full report of detected difficulties"""

    overall_skill: float  # 0.0 to 1.0
    skills: Dict[str, SkillAssessment]
    struggles: List[Dict]  # Current struggles detected
    improvements: List[Dict]  # Areas of improvement
    regressions: List[Dict]  # Areas getting worse
    recommendations: List[str]  # What to focus on


class DifficultyDetector:
    """
    Analyzes user behavior to detect struggles and skill levels.

    Skill areas tracked:
    - search_ability: Can they find what they need?
    - naming_consistency: Do they use consistent naming?
    - folder_organization: Are files logically organized?
    - file_management: Can they manage files effectively?
    """

    # Minimum events needed for confident assessment
    MIN_SAMPLES = 10

    # Skill score thresholds
    THRESHOLD_STRUGGLING = 0.4
    THRESHOLD_LEARNING = 0.7
    THRESHOLD_PROFICIENT = 0.85

    def __init__(self, tracker: BehaviorTracker):
        self.tracker = tracker
        self.skill_history = defaultdict(list)  # Track scores over time

    def assess_all_skills(self) -> DifficultyReport:
        """Run full skill assessment across all areas"""

        skills = {
            "search_ability": self._assess_search_ability(),
            "naming_consistency": self._assess_naming_consistency(),
            "folder_organization": self._assess_folder_organization(),
            "file_management": self._assess_file_management(),
        }

        # Calculate overall skill
        valid_skills = [s for s in skills.values() if s.confidence > 0.3]
        if valid_skills:
            overall = sum(s.score * s.confidence for s in valid_skills) / sum(
                s.confidence for s in valid_skills
            )
        else:
            overall = 0.5  # Unknown

        # Identify struggles, improvements, regressions
        struggles = self._identify_struggles(skills)
        improvements = self._identify_improvements(skills)
        regressions = self._identify_regressions(skills)

        # Generate recommendations
        recommendations = self._generate_recommendations(skills, struggles)

        return DifficultyReport(
            overall_skill=overall,
            skills=skills,
            struggles=struggles,
            improvements=improvements,
            regressions=regressions,
            recommendations=recommendations,
        )

    def _assess_search_ability(self) -> SkillAssessment:
        """Assess user's ability to find files via search"""
        patterns = self.tracker.get_search_patterns(days=30)

        total = patterns["total_searches"]
        if total < 3:
            return SkillAssessment(
                skill_name="search_ability",
                score=0.5,
                confidence=0.1,
                trend="new",
                evidence=["Not enough search data"],
                last_updated=datetime.now().isoformat(),
                samples_count=total,
            )

        evidence = []

        # Success rate is primary indicator
        success_rate = patterns["success_rate"]
        evidence.append(f"Search success rate: {success_rate:.0%}")

        # Refinements indicate struggle
        refinement_rate = len(patterns["search_refinements"]) / total
        evidence.append(f"Search refinement rate: {refinement_rate:.0%}")

        # Failed queries
        failure_rate = len(patterns["failed_queries"]) / total
        evidence.append(f"Abandoned searches: {failure_rate:.0%}")

        # Calculate score
        # High success = good, low refinements = good, low failures = good
        score = (
            success_rate * 0.5
            + (1 - refinement_rate) * 0.25
            + (1 - failure_rate) * 0.25
        )

        # Determine trend
        trend = self._calculate_trend("search_ability", score)

        return SkillAssessment(
            skill_name="search_ability",
            score=score,
            confidence=min(total / 20, 1.0),  # More samples = more confidence
            trend=trend,
            evidence=evidence,
            last_updated=datetime.now().isoformat(),
            samples_count=total,
        )

    def _assess_naming_consistency(self) -> SkillAssessment:
        """Assess how consistent user's file naming is"""
        naming_data = self.tracker.get_naming_preferences()
        file_patterns = self.tracker.get_file_patterns(days=30)

        sample_size = naming_data.get("sample_size", 0)
        if sample_size < 5:
            return SkillAssessment(
                skill_name="naming_consistency",
                score=0.5,
                confidence=0.1,
                trend="new",
                evidence=["Not enough naming data"],
                last_updated=datetime.now().isoformat(),
                samples_count=sample_size,
            )

        evidence = []
        patterns = naming_data.get("learned_patterns", {})

        # Check for consistent separator usage
        underscore_pct = patterns.get("uses_underscores", 0) / sample_size
        hyphen_pct = patterns.get("uses_hyphens", 0) / sample_size
        separator_consistency = max(
            underscore_pct, hyphen_pct, 1 - underscore_pct - hyphen_pct
        )
        evidence.append(f"Separator consistency: {separator_consistency:.0%}")

        # Check for consistent case usage
        lowercase_pct = patterns.get("uses_lowercase", 0) / sample_size
        case_consistency = max(lowercase_pct, 1 - lowercase_pct)
        evidence.append(f"Case consistency: {case_consistency:.0%}")

        # Check for prefix usage (indicates categorization)
        prefixes = patterns.get("uses_prefixes", [])
        has_prefixes = len(prefixes) > 0 and prefixes[0].get("count", 0) >= 3
        evidence.append(f"Uses category prefixes: {'Yes' if has_prefixes else 'No'}")

        # Calculate score
        score = (
            separator_consistency * 0.4
            + case_consistency * 0.3
            + (0.3 if has_prefixes else 0.15)
        )

        trend = self._calculate_trend("naming_consistency", score)

        return SkillAssessment(
            skill_name="naming_consistency",
            score=score,
            confidence=min(sample_size / 15, 1.0),
            trend=trend,
            evidence=evidence,
            last_updated=datetime.now().isoformat(),
            samples_count=sample_size,
        )

    def _assess_folder_organization(self) -> SkillAssessment:
        """Assess how well user organizes files into folders"""
        # Analyze navigation patterns for signs of disorganization
        events = self.tracker.events.get("navigation", [])
        file_accesses = self.tracker.events.get("file_accesses", [])

        if len(events) < 5:
            return SkillAssessment(
                skill_name="folder_organization",
                score=0.5,
                confidence=0.1,
                trend="new",
                evidence=["Not enough navigation data"],
                last_updated=datetime.now().isoformat(),
                samples_count=len(events),
            )

        evidence = []

        # Long navigation times suggest disorganization
        avg_nav_time = sum(e.get("time_spent_seconds", 0) for e in events[-20:]) / min(
            len(events), 20
        )
        time_score = 1.0 - min(avg_nav_time / 60, 1.0)  # Over 60 seconds is concerning
        evidence.append(f"Avg navigation time: {avg_nav_time:.1f}s")

        # Deep paths suggest poor organization
        paths = [
            a.get("file_path", "") for a in file_accesses[-50:] if a.get("file_path")
        ]
        if paths:
            avg_depth = sum(len(Path(p).parts) for p in paths) / len(paths)
            depth_score = 1.0 - min(
                (avg_depth - 3) / 7, 1.0
            )  # 3 levels is normal, 10+ is bad
            evidence.append(f"Avg folder depth: {avg_depth:.1f}")
        else:
            depth_score = 0.5

        # File scatter (same types in many different folders)
        folder_to_types = defaultdict(set)
        for access in file_accesses[-100:]:
            path = access.get("file_path", "")
            if path:
                folder = str(Path(path).parent)
                file_type = access.get("file_type", "")
                folder_to_types[folder].add(file_type)

        # Many folders with same file type = scattered
        if folder_to_types:
            type_folders = defaultdict(int)
            for folder, types in folder_to_types.items():
                for t in types:
                    type_folders[t] += 1
            max_scatter = max(type_folders.values()) if type_folders else 1
            scatter_score = 1.0 - min(
                (max_scatter - 3) / 10, 1.0
            )  # Up to 3 folders per type is OK
            evidence.append(f"Max file type scatter: {max_scatter} folders")
        else:
            scatter_score = 0.5

        score = time_score * 0.3 + depth_score * 0.35 + scatter_score * 0.35
        trend = self._calculate_trend("folder_organization", score)

        return SkillAssessment(
            skill_name="folder_organization",
            score=score,
            confidence=min(len(events) / 20, 1.0),
            trend=trend,
            evidence=evidence,
            last_updated=datetime.now().isoformat(),
            samples_count=len(events),
        )

    def _assess_file_management(self) -> SkillAssessment:
        """Assess overall file management skills"""
        file_patterns = self.tracker.get_file_patterns(days=30)
        suggestion_effectiveness = self.tracker.get_suggestion_effectiveness()

        total = file_patterns["total_accesses"]
        if total < 5:
            return SkillAssessment(
                skill_name="file_management",
                score=0.5,
                confidence=0.1,
                trend="new",
                evidence=["Not enough file access data"],
                last_updated=datetime.now().isoformat(),
                samples_count=total,
            )

        evidence = []

        # Rename frequency (low is better - means files named well initially)
        renames = file_patterns["by_access_type"].get("rename", 0)
        opens = file_patterns["by_access_type"].get("open", 1)
        rename_ratio = renames / max(opens, 1)
        rename_score = 1.0 - min(
            rename_ratio / 0.3, 1.0
        )  # 30% rename ratio is very high
        evidence.append(f"Rename ratio: {rename_ratio:.0%}")

        # Suggestion acceptance (shows learning)
        if suggestion_effectiveness["total_suggestions"] > 0:
            acceptance = suggestion_effectiveness["acceptance_rate"]
            customization = suggestion_effectiveness["customization_rate"]
            # Both acceptance and thoughtful customization are good
            suggestion_score = acceptance * 0.7 + customization * 0.3
            evidence.append(f"Suggestion acceptance: {acceptance:.0%}")
            evidence.append(f"Suggestion customization: {customization:.0%}")
        else:
            suggestion_score = 0.5  # Unknown

        # Variety of operations (experienced users do more operations)
        operation_types = len(
            [v for v in file_patterns["by_access_type"].values() if v > 0]
        )
        variety_score = min(
            operation_types / 4, 1.0
        )  # 4+ operation types is experienced
        evidence.append(f"Operation variety: {operation_types} types")

        score = rename_score * 0.4 + suggestion_score * 0.35 + variety_score * 0.25
        trend = self._calculate_trend("file_management", score)

        return SkillAssessment(
            skill_name="file_management",
            score=score,
            confidence=min(total / 30, 1.0),
            trend=trend,
            evidence=evidence,
            last_updated=datetime.now().isoformat(),
            samples_count=total,
        )

    def _calculate_trend(self, skill_name: str, current_score: float) -> str:
        """Determine if skill is improving, stable, or regressing"""
        history = self.skill_history[skill_name]

        # Store current score
        history.append(
            {"score": current_score, "timestamp": datetime.now().isoformat()}
        )

        # Keep only last 10 assessments
        self.skill_history[skill_name] = history[-10:]

        if len(history) < 3:
            return "new"

        # Compare recent (last 3) vs older (before that)
        recent = [h["score"] for h in history[-3:]]
        older = [h["score"] for h in history[:-3]] if len(history) > 3 else recent

        recent_avg = sum(recent) / len(recent)
        older_avg = sum(older) / len(older)

        diff = recent_avg - older_avg

        if diff > 0.1:
            return "improving"
        elif diff < -0.1:
            return "regressing"
        else:
            return "stable"

    def _identify_struggles(self, skills: Dict[str, SkillAssessment]) -> List[Dict]:
        """Identify current areas of struggle"""
        struggles = []

        for name, assessment in skills.items():
            if (
                assessment.score < self.THRESHOLD_STRUGGLING
                and assessment.confidence > 0.3
            ):
                struggles.append(
                    {
                        "skill": name,
                        "score": assessment.score,
                        "evidence": assessment.evidence,
                        "severity": "high" if assessment.score < 0.25 else "medium",
                    }
                )

        return sorted(struggles, key=lambda x: x["score"])

    def _identify_improvements(self, skills: Dict[str, SkillAssessment]) -> List[Dict]:
        """Identify areas where user is improving"""
        improvements = []

        for name, assessment in skills.items():
            if assessment.trend == "improving":
                improvements.append(
                    {
                        "skill": name,
                        "current_score": assessment.score,
                        "message": f"Great progress on {name.replace('_', ' ')}!",
                    }
                )

        return improvements

    def _identify_regressions(self, skills: Dict[str, SkillAssessment]) -> List[Dict]:
        """Identify areas getting worse (need to re-engage)"""
        regressions = []

        for name, assessment in skills.items():
            if assessment.trend == "regressing" and assessment.confidence > 0.4:
                regressions.append(
                    {
                        "skill": name,
                        "current_score": assessment.score,
                        "message": f"Noticed some regression in {name.replace('_', ' ')}. Let me help!",
                        "should_increase_suggestions": True,
                    }
                )

        return regressions

    def _generate_recommendations(
        self, skills: Dict[str, SkillAssessment], struggles: List[Dict]
    ) -> List[str]:
        """Generate actionable recommendations"""
        recommendations = []

        # Prioritize struggles
        for struggle in struggles[:2]:  # Max 2 recommendations at a time
            skill = struggle["skill"]

            if skill == "search_ability":
                recommendations.append(
                    "Try using more specific keywords when searching. "
                    "I'll show preview snippets to help you identify the right files."
                )

            elif skill == "naming_consistency":
                recommendations.append(
                    "I noticed varying naming patterns. Consider using a consistent format like: "
                    "CATEGORY_topic_YYYYMMDD (e.g., NOTES_meeting_20260209)"
                )

            elif skill == "folder_organization":
                recommendations.append(
                    "Some files seem scattered. Would you like me to suggest a folder structure "
                    "based on your file types and topics?"
                )

            elif skill == "file_management":
                recommendations.append(
                    "You're renaming files frequently. I can suggest better names upfront "
                    "to save you time."
                )

        # Add encouraging message for improvements
        improving = [s for s in skills.values() if s.trend == "improving"]
        if improving:
            skill_name = improving[0].skill_name.replace("_", " ")
            recommendations.append(
                f"You're getting better at {skill_name}! Keep it up."
            )

        return recommendations

    def should_show_suggestions(self, skill_name: str) -> Tuple[bool, float]:
        """
        Determine if suggestions should be shown for this skill area.
        Returns (should_show, intensity) where intensity is 0.0 to 1.0

        Logic:
        - Low skill + high confidence = SHOW with high intensity
        - High skill + stable trend = DON'T show (fade out)
        - High skill + regressing trend = SHOW again (catch regression)
        - New user = SHOW with medium intensity (onboarding)
        """
        report = self.assess_all_skills()

        if skill_name not in report.skills:
            return True, 0.5  # Unknown skill, show with medium intensity

        assessment = report.skills[skill_name]

        # New user - show suggestions gently
        if assessment.trend == "new":
            return True, 0.5

        # Regression detected - re-engage
        if assessment.trend == "regressing":
            # Intensity based on how bad the regression is
            intensity = 1.0 - assessment.score
            return True, min(intensity * 1.2, 1.0)  # Boost intensity for regressions

        # Struggling - definitely show
        if assessment.score < self.THRESHOLD_STRUGGLING:
            intensity = 1.0 - assessment.score  # Lower score = higher intensity
            return True, intensity

        # Learning - show with decreasing intensity
        if assessment.score < self.THRESHOLD_LEARNING:
            intensity = (self.THRESHOLD_LEARNING - assessment.score) / 0.3
            return True, intensity

        # Proficient - minimal suggestions
        if assessment.score < self.THRESHOLD_PROFICIENT:
            intensity = (self.THRESHOLD_PROFICIENT - assessment.score) / 0.15
            return True, intensity * 0.5  # Halve intensity for proficient users

        # Mastered - don't show unless asked
        return False, 0.0

    def get_suggestion_intensity(self) -> Dict[str, float]:
        """Get suggestion intensity for all skills"""
        intensities = {}
        for skill in [
            "search_ability",
            "naming_consistency",
            "folder_organization",
            "file_management",
        ]:
            show, intensity = self.should_show_suggestions(skill)
            intensities[skill] = intensity if show else 0.0
        return intensities
