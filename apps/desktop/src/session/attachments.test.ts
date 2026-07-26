import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  audioDelete: vi.fn(),
  audioMetadata: vi.fn(),
  sessionStoreAudio: vi.fn(),
  sessionListAudio: vi.fn(),
  sessionDeleteAudio: vi.fn(),
  enqueueDatabaseWrite: vi.fn(
    async (_key: string, write: () => Promise<number[]>) => write(),
  ),
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    audioDelete: mocks.audioDelete,
    audioMetadata: mocks.audioMetadata,
  },
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionStoreAudio: mocks.sessionStoreAudio,
    sessionListAudio: mocks.sessionListAudio,
    sessionDeleteAudio: mocks.sessionDeleteAudio,
  },
}));

vi.mock("~/shared/write-queue", () => ({
  enqueueDatabaseWrite: mocks.enqueueDatabaseWrite,
}));

import {
  catalogLocalNoteAttachment,
  catalogLocalSessionAudio,
  cleanupDeletedSessionAudio,
  deleteLocalSessionAudio,
  deleteSessionAudio,
  sha256Hex,
} from "./attachments";

describe("attachment catalog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: true });
    mocks.audioMetadata.mockResolvedValue({
      status: "ok",
      data: {
        filename: "audio.mp3",
        contentType: "audio/mpeg",
        sizeBytes: 84,
        sha256: "d".repeat(64),
      },
    });
    mocks.sessionStoreAudio.mockResolvedValue({
      status: "ok",
      data: "sessions/session-1/audio/recording.wav",
    });
    mocks.sessionListAudio.mockResolvedValue({ status: "ok", data: [] });
    mocks.sessionDeleteAudio.mockResolvedValue({ status: "ok", data: null });
  });

  // catalogLocalNoteAttachment is a graceful no-op pending a file-canonical
  // home for note attachments (deferred past Task 9) — session_attachments/
  // attachment_local_state were dropped in Task 4. See the comment above its
  // definition in attachments.ts.
  it("resolves without touching the database", async () => {
    await expect(
      catalogLocalNoteAttachment({
        sessionId: "session-1",
        attachmentId: "diagram 1.png",
        filename: "diagram.png",
        contentType: "image/png",
        sizeBytes: 42,
        sha256: "a".repeat(64),
      }),
    ).resolves.toBeUndefined();

    expect(mocks.enqueueDatabaseWrite).not.toHaveBeenCalled();
  });

  it("computes a stable lowercase SHA-256 checksum", async () => {
    const bytes = new TextEncoder().encode("hello").buffer;

    await expect(sha256Hex(bytes)).resolves.toBe(
      "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
  });

  it("moves a finished recording into the session's audio folder via the store", async () => {
    await expect(
      catalogLocalSessionAudio("session-1", "/tmp/recording.wav"),
    ).resolves.toBeUndefined();

    expect(mocks.sessionStoreAudio).toHaveBeenCalledWith(
      "session-1",
      "/tmp/recording.wav",
    );
  });

  it("surfaces a store failure instead of swallowing it", async () => {
    mocks.sessionStoreAudio.mockResolvedValue({
      status: "error",
      error: "disk full",
    });

    await expect(
      catalogLocalSessionAudio("session-1", "/tmp/recording.wav"),
    ).rejects.toThrow("disk full");
  });

  // markSessionAudioAvailability/tombstoneSessionAudioMetadata are a
  // graceful no-op — attachment_local_state/session_attachments were
  // dropped in Task 4. The on-disk file deletion they accompany (both the
  // legacy flat layout and the store-owned audio/ folder) keeps working
  // unchanged. See the comments above their definitions in attachments.ts.
  it("deletes local audio bytes from both the flat and store-owned locations", async () => {
    mocks.sessionListAudio.mockResolvedValue({
      status: "ok",
      data: ["recording.wav"],
    });

    await expect(
      deleteLocalSessionAudio("session-1", () => true),
    ).resolves.toBe(true);
    expect(mocks.audioDelete).toHaveBeenCalledWith("session-1");
    expect(mocks.sessionListAudio).toHaveBeenCalledWith("session-1");
    expect(mocks.sessionDeleteAudio).toHaveBeenCalledWith(
      "session-1",
      "recording.wav",
    );
  });

  it("deletes the recording without touching the database", async () => {
    await expect(deleteSessionAudio("session-1", () => true)).resolves.toBe(
      true,
    );
    expect(mocks.audioDelete).toHaveBeenCalledWith("session-1");
  });

  it("completes logical deletion when local audio is already absent", async () => {
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: false });

    await expect(deleteSessionAudio("session-1", () => true)).resolves.toBe(
      true,
    );
  });

  it("resolves false when retention finds no local audio to delete", async () => {
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: false });

    await expect(
      deleteLocalSessionAudio("session-1", () => true),
    ).resolves.toBe(false);
  });

  // cleanupDeletedSessionAudio is likewise a graceful no-op — there is no
  // longer a way to detect a logically-deleted-but-locally-present
  // attachment to retry cleanup on. See the comment above its definition.
  it("resolves without deleting anything (backing table dropped)", async () => {
    await expect(
      cleanupDeletedSessionAudio("session-1", () => true),
    ).resolves.toBe(false);
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });

  it("rechecks capture safety inside the serialized delete operation", async () => {
    await expect(deleteSessionAudio("session-1", () => false)).resolves.toBe(
      false,
    );
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });
});
