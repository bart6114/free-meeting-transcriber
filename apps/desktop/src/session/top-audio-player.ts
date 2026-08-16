export function shouldShowSessionTopAudioPlayer({
  audioExists,
  audioUrlReady,
  sessionMode,
}: {
  audioExists: boolean;
  audioUrlReady: boolean;
  sessionMode: string;
}) {
  return (
    audioExists &&
    audioUrlReady &&
    sessionMode !== "active" &&
    sessionMode !== "finalizing"
  );
}
