"""Closed-form checks of frame-pacing statistics and malformed input rejection."""
import unittest

from analyze_frame_capture import summarize


def capture(values):
    return {
        "format": "ancient-earth-frame-capture-v1",
        "sample_count": len(values),
        "recorded_duration_seconds": sum(values),
        "interval_seconds": values,
        "view_changed_during_capture": False,
        "incomplete_scene_frames": 0,
        "unfocused_recorded_frames": 0,
    }


class FrameCaptureAnalysisTests(unittest.TestCase):
    def test_throughput_uses_total_time_not_mean_instantaneous_fps(self):
        result = summarize(capture([0.01, 0.03]))
        self.assertAlmostEqual(result["throughput_frames_per_second"], 50.0)
        self.assertAlmostEqual(result["frame_interval_ms"]["mean"], 20.0)
        self.assertAlmostEqual(result["frame_interval_ms"]["median"], 20.0)
        self.assertAlmostEqual(result["frame_interval_ms"]["p95_nearest_rank"], 30.0)
        self.assertTrue(result["eligible_as_stationary_scene_baseline"])
        incomplete = capture([0.01, 5.0])
        incomplete["incomplete_scene_frames"] = 1
        measured = summarize(incomplete)
        self.assertFalse(measured["eligible_as_stationary_scene_baseline"])
        self.assertEqual(measured["frame_interval_ms"]["maximum"], 5000.0)
        unfocused = capture([0.01, 0.03])
        unfocused["unfocused_recorded_frames"] = 1
        self.assertFalse(summarize(unfocused)["eligible_as_stationary_scene_baseline"])

    def test_rank_boundaries_are_exact_and_input_order_does_not_matter(self):
        result = summarize(capture([i / 1000 for i in range(100, 0, -1)]))
        timing = result["frame_interval_ms"]
        self.assertAlmostEqual(timing["median"], 50.5)
        self.assertAlmostEqual(timing["p95_nearest_rank"], 95.0)
        self.assertAlmostEqual(timing["p99_nearest_rank"], 99.0)
        single = summarize(capture([0.02]))["frame_interval_ms"]
        self.assertTrue(all(abs(value - 20.0) < 1e-12 for value in single.values()))

    def test_invalid_or_inconsistent_raw_samples_are_rejected(self):
        for malformed in [None, [], "capture", 0]:
            with self.subTest(malformed=malformed), self.assertRaises(ValueError):
                summarize(malformed)
        for values in [[], [0], [-0.1], [float("nan")], [float("inf")], [True]]:
            with self.subTest(values=values), self.assertRaises(ValueError):
                summarize(capture(values))
        wrong_count = capture([0.01])
        for invalid_count in [2, True, 1.0]:
            wrong_count["sample_count"] = invalid_count
            with self.assertRaises(ValueError):
                summarize(wrong_count)
        wrong_duration = capture([0.01])
        wrong_duration["recorded_duration_seconds"] = 1.0
        with self.assertRaises(ValueError):
            summarize(wrong_duration)
        with self.assertRaises(ValueError):
            summarize(capture([1e308]))


if __name__ == "__main__":
    unittest.main()
