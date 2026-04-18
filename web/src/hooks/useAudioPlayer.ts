import {
  createContext,
  useContext,
  useState,
  useRef,
  useCallback,
  useEffect,
  type ReactNode,
} from "react";
import { createElement } from "react";
import { api } from "../api/client";
import { DEFAULT_VIDEO_POSTER_URL } from "../assets";

export interface AudioTrack {
  title: string;
  artist?: string;
  album?: string;
  artworkUrl?: string;
  streamId: string;
  fileIndex?: number;
  format?: string;
  fileSize?: number;
}

interface AudioPlayerState {
  currentTrack: AudioTrack | null;
  isPlaying: boolean;
  duration: number;
  currentTime: number;
  queue: AudioTrack[];
  queueIndex: number;
  repeat: "none" | "all" | "one";
  play: (track: AudioTrack) => void;
  playQueue: (tracks: AudioTrack[], startIndex?: number) => void;
  pause: () => void;
  resume: () => void;
  stop: () => void;
  seek: (time: number) => void;
  next: () => void;
  previous: () => void;
  toggleRepeat: () => void;
  audioRef: React.RefObject<HTMLAudioElement | null>;
}

const PLAYER_STATE_KEY = "streamx_player_state";

interface SavedPlayerState {
  track: AudioTrack;
  queue: AudioTrack[];
  queueIndex: number;
  currentTime: number;
}

function savePlayerState(state: SavedPlayerState) {
  try { localStorage.setItem(PLAYER_STATE_KEY, JSON.stringify(state)); } catch { /* quota */ }
}

function loadPlayerState(): SavedPlayerState | null {
  try {
    const saved = localStorage.getItem(PLAYER_STATE_KEY);
    if (saved) return JSON.parse(saved);
  } catch { /* ignore */ }
  return null;
}

// Global flag to pause React time updates during gestures (set by ExpandedPlayer drag)
let _timeUpdatesPaused = false;
export function pauseTimeUpdates() { _timeUpdatesPaused = true; }
export function resumeTimeUpdates() { _timeUpdatesPaused = false; }

function clearPlayerState() {
  try { localStorage.removeItem(PLAYER_STATE_KEY); } catch { /* ignore */ }
}

const AudioPlayerContext = createContext<AudioPlayerState | null>(null);

export function AudioPlayerProvider({ children }: { children: ReactNode }) {
  const [currentTrack, setCurrentTrack] = useState<AudioTrack | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [queue, setQueue] = useState<AudioTrack[]>([]);
  const [queueIndex, setQueueIndex] = useState(-1);
  const [repeat, setRepeat] = useState<"none" | "all" | "one">("none");
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const currentTrackRef = useRef<AudioTrack | null>(null);
  const queueRef = useRef<AudioTrack[]>([]);
  const queueIndexRef = useRef(-1);
  const repeatRef = useRef<"none" | "all" | "one">("none");
  const savedTimeRef = useRef(0);

  // Keep refs in sync for use in callbacks
  useEffect(() => { currentTrackRef.current = currentTrack; }, [currentTrack]);
  useEffect(() => { queueRef.current = queue; }, [queue]);
  useEffect(() => { queueIndexRef.current = queueIndex; }, [queueIndex]);
  useEffect(() => { repeatRef.current = repeat; }, [repeat]);

  const loadTrack = useCallback((track: AudioTrack) => {
    const audio = audioRef.current;
    if (!audio) return;

    const fileUrl = track.fileIndex !== undefined
      ? api.getFileByIndexUrl(track.streamId, track.fileIndex)
      : api.getFileUrl(track.streamId);
    audio.src = fileUrl;
    audio.play().catch(() => {});
    setCurrentTrack(track);
    setCurrentTime(0);
    setDuration(0);
  }, []);

  // Create audio element once
  useEffect(() => {
    const audio = new Audio();
    audio.preload = "auto";
    audio.setAttribute("x-webkit-airplay", "allow");
    audio.setAttribute("airplay", "allow");
    audioRef.current = audio;

    // Throttle time updates to 1/sec to avoid re-render jank during gestures
    let lastTimeUpdate = 0;
    let lastSave = 0;
    let timeRaf = 0;
    const onTimeUpdate = () => {
      if (_timeUpdatesPaused) return;
      const now = Date.now();
      if (now - lastTimeUpdate < 900) return;
      lastTimeUpdate = now;
      cancelAnimationFrame(timeRaf);
      timeRaf = requestAnimationFrame(() => {
        if (!_timeUpdatesPaused) setCurrentTime(audio.currentTime);
      });
      if (now - lastSave > 5000) {
        lastSave = now;
        const track = currentTrackRef.current;
        if (track) {
          savePlayerState({
            track,
            queue: queueRef.current,
            queueIndex: queueIndexRef.current,
            currentTime: audio.currentTime,
          });
        }
      }
    };
    const onDurationChange = () => setDuration(audio.duration || 0);
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onEnded = () => {
      setIsPlaying(false);
      const q = queueRef.current;
      const idx = queueIndexRef.current;
      const rep = repeatRef.current;

      if (rep === "one") {
        audio.currentTime = 0;
        audio.play().catch(() => {});
        return;
      }

      if (idx >= 0 && idx < q.length - 1) {
        // Next track in queue
        const nextIdx = idx + 1;
        setQueueIndex(nextIdx);
        queueIndexRef.current = nextIdx;
        const nextTrack = q[nextIdx];
        if (nextTrack) {
          const url = nextTrack.fileIndex !== undefined
            ? api.getFileByIndexUrl(nextTrack.streamId, nextTrack.fileIndex)
            : api.getFileUrl(nextTrack.streamId);
          audio.src = url;
          audio.play().catch(() => {});
          setCurrentTrack(nextTrack);
          setCurrentTime(0);
          setDuration(0);
        }
      } else if (rep === "all" && q.length > 0) {
        // Loop back to start
        setQueueIndex(0);
        queueIndexRef.current = 0;
        const firstTrack = q[0];
        if (firstTrack) {
          const url = firstTrack.fileIndex !== undefined
            ? api.getFileByIndexUrl(firstTrack.streamId, firstTrack.fileIndex)
            : api.getFileUrl(firstTrack.streamId);
          audio.src = url;
          audio.play().catch(() => {});
          setCurrentTrack(firstTrack);
          setCurrentTime(0);
          setDuration(0);
        }
      }
    };

    audio.addEventListener("timeupdate", onTimeUpdate);
    audio.addEventListener("durationchange", onDurationChange);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);

    // Restore saved playback state (paused, shows last track without auto-playing)
    const saved = loadPlayerState();
    if (saved?.track) {
      setCurrentTrack(saved.track);
      currentTrackRef.current = saved.track;
      setQueue(saved.queue);
      setQueueIndex(saved.queueIndex);
      queueRef.current = saved.queue;
      queueIndexRef.current = saved.queueIndex;
      setCurrentTime(saved.currentTime);
      savedTimeRef.current = saved.currentTime;
      setDuration(saved.currentTime > 0 ? saved.currentTime * 1.1 : 0); // approximate until real duration loads
      // Don't set audio.src - avoids auto-loading/playing
      // Track will load when user taps play via the resume/play callback
    }

    return () => {
      audio.removeEventListener("timeupdate", onTimeUpdate);
      audio.removeEventListener("durationchange", onDurationChange);
      audio.removeEventListener("ended", onEnded);
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.pause();
      audio.src = "";
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // MediaSession API
  useEffect(() => {
    if (!currentTrack || !("mediaSession" in navigator)) return;

    navigator.mediaSession.metadata = new MediaMetadata({
      title: currentTrack.title,
      artist: currentTrack.artist ?? "",
      album: currentTrack.album ?? "",
      artwork: [
        ...(currentTrack.artworkUrl ? [{ src: currentTrack.artworkUrl, sizes: "512x512", type: "image/jpeg" }] : []),
        { src: DEFAULT_VIDEO_POSTER_URL, sizes: "512x512", type: "image/jpeg" },
      ],
    });

    navigator.mediaSession.setActionHandler("play", () => {
      audioRef.current?.play();
    });
    navigator.mediaSession.setActionHandler("pause", () => {
      audioRef.current?.pause();
    });
    navigator.mediaSession.setActionHandler("seekto", (details) => {
      if (audioRef.current && details.seekTime != null) {
        audioRef.current.currentTime = details.seekTime;
      }
    });
    navigator.mediaSession.setActionHandler("nexttrack", () => {
      const q = queueRef.current;
      const idx = queueIndexRef.current;
      if (idx >= 0 && idx < q.length - 1) {
        const nextIdx = idx + 1;
        setQueueIndex(nextIdx);
        queueIndexRef.current = nextIdx;
        const t = q[nextIdx];
        if (t) loadTrack(t);
      }
    });
    navigator.mediaSession.setActionHandler("previoustrack", () => {
      const audio = audioRef.current;
      if (audio && audio.currentTime > 3) {
        audio.currentTime = 0;
        return;
      }
      const q = queueRef.current;
      const idx = queueIndexRef.current;
      if (idx > 0) {
        const prevIdx = idx - 1;
        setQueueIndex(prevIdx);
        queueIndexRef.current = prevIdx;
        const t = q[prevIdx];
        if (t) loadTrack(t);
      }
    });
  }, [currentTrack, loadTrack]);

  const play = useCallback((track: AudioTrack) => {
    setQueue([track]);
    setQueueIndex(0);
    queueRef.current = [track];
    queueIndexRef.current = 0;
    loadTrack(track);
  }, [loadTrack]);

  const playQueue = useCallback((tracks: AudioTrack[], startIndex = 0) => {
    setQueue(tracks);
    setQueueIndex(startIndex);
    queueRef.current = tracks;
    queueIndexRef.current = startIndex;
    const track = tracks[startIndex];
    if (track) loadTrack(track);
  }, [loadTrack]);

  const pause = useCallback(() => { audioRef.current?.pause(); }, []);
  const resume = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    // If src is empty (restored from localStorage), load the track first
    const track = currentTrackRef.current;
    if ((!audio.src || audio.src === window.location.href) && track) {
      const url = track.fileIndex !== undefined
        ? api.getFileByIndexUrl(track.streamId, track.fileIndex)
        : api.getFileUrl(track.streamId);
      const resumeTime = savedTimeRef.current;
      audio.src = url;
      const onLoaded = () => {
        if (resumeTime > 0) audio.currentTime = resumeTime;
        audio.play().catch(() => {});
        audio.removeEventListener("loadedmetadata", onLoaded);
      };
      audio.addEventListener("loadedmetadata", onLoaded);
    } else {
      audio.play().catch(() => {});
    }
  }, []);

  const stop = useCallback(() => {
    const audio = audioRef.current;
    if (audio) { audio.pause(); audio.src = ""; }
    setCurrentTrack(null);
    setIsPlaying(false);
    setCurrentTime(0);
    setDuration(0);
    setQueue([]);
    setQueueIndex(-1);
    clearPlayerState();
  }, []);

  const seek = useCallback((time: number) => {
    if (audioRef.current) audioRef.current.currentTime = time;
  }, []);

  const next = useCallback(() => {
    const q = queueRef.current;
    const idx = queueIndexRef.current;
    if (idx >= 0 && idx < q.length - 1) {
      const nextIdx = idx + 1;
      setQueueIndex(nextIdx);
      queueIndexRef.current = nextIdx;
      const t = q[nextIdx];
      if (t) loadTrack(t);
    }
  }, [loadTrack]);

  const previous = useCallback(() => {
    const audio = audioRef.current;
    if (audio && audio.currentTime > 3) { audio.currentTime = 0; return; }
    const q = queueRef.current;
    const idx = queueIndexRef.current;
    if (idx > 0) {
      const prevIdx = idx - 1;
      setQueueIndex(prevIdx);
      queueIndexRef.current = prevIdx;
      const t = q[prevIdx];
      if (t) loadTrack(t);
    }
  }, [loadTrack]);

  const toggleRepeat = useCallback(() => {
    setRepeat((r) => {
      const next = r === "none" ? "all" : r === "all" ? "one" : "none";
      repeatRef.current = next;
      return next;
    });
  }, []);

  return createElement(
    AudioPlayerContext.Provider,
    {
      value: {
        currentTrack, isPlaying, duration, currentTime,
        queue, queueIndex, repeat,
        play, playQueue, pause, resume, stop, seek, next, previous, toggleRepeat,
        audioRef,
      },
    },
    children
  );
}

export function useAudioPlayer(): AudioPlayerState {
  const ctx = useContext(AudioPlayerContext);
  if (!ctx) throw new Error("useAudioPlayer must be used within AudioPlayerProvider");
  return ctx;
}
