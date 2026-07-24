import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  audioDelete: vi.fn(),
  audioMetadata: vi.fn(),
  execute: vi.fn(),
  executeTransaction: vi.fn().mockResolvedValue([0, 1, 1]),
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

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
  liveQueryClient: { execute: mocks.execute },
}));

vi.mock("~/db/write-queue", () => ({
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
    mocks.executeTransaction.mockResolvedValue([0, 1, 1]);
    mocks.execute.mockResolvedValue([{ is_deleted: 1 }]);
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
  });

  // catalogLocalNoteAttachment/catalogLocalSessionAudio are a graceful no-op
  // pending Task 9 (Session store scaffold) — session_attachments/
  // attachment_local_state were dropped in Task 4. See the comment above
  // their definitions in attachments.ts.
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
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("computes a stable lowercase SHA-256 checksum", async () => {
    const bytes = new TextEncoder().encode("hello").buffer;

    await expect(sha256Hex(bytes)).resolves.toBe(
      "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
  });

  it("resolves without querying audio metadata or the database", async () => {
    await expect(
      catalogLocalSessionAudio("session-1"),
    ).resolves.toBeUndefined();

    expect(mocks.audioMetadata).not.toHaveBeenCalled();
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("keeps canonical metadata when retention deletes only local audio bytes", async () => {
    await expect(
      deleteLocalSessionAudio("session-1", () => true),
    ).resolves.toBe(true);
    expect(mocks.audioDelete).toHaveBeenCalledWith("session-1");
    const localState = mocks.executeTransaction.mock.calls[0]![0][0];
    expect(localState.sql).toContain("attachment_local_state");
    expect(localState.params).toEqual([
      "session-audio:session-1",
      "session-1",
      "absent",
    ]);
    expect(localState.sql).not.toContain("UPDATE session_attachments");
  });

  it("tombstones logical audio before deleting local bytes", async () => {
    mocks.executeTransaction.mockResolvedValue([1]);

    await expect(deleteSessionAudio("session-1", () => true)).resolves.toBe(
      true,
    );

    expect(mocks.executeTransaction.mock.calls[0]![0][0].params).toEqual([
      "session-audio:session-1",
      "session-1",
    ]);
    expect(mocks.executeTransaction.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.audioDelete.mock.invocationCallOrder[0]!,
    );
    expect(mocks.audioDelete.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.executeTransaction.mock.invocationCallOrder[1]!,
    );
  });

  it("does not delete bytes when the logical tombstone fails", async () => {
    mocks.executeTransaction.mockRejectedValueOnce(
      new Error("database locked"),
    );

    await expect(deleteSessionAudio("session-1", () => true)).rejects.toThrow(
      "database locked",
    );
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });

  it("completes logical deletion when local audio is already absent", async () => {
    mocks.executeTransaction.mockResolvedValue([1]);
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: false });

    await expect(deleteSessionAudio("session-1", () => true)).resolves.toBe(
      true,
    );
    expect(mocks.executeTransaction).toHaveBeenCalledTimes(2);
  });

  it("records local absence when retention finds no local audio", async () => {
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: false });

    await expect(
      deleteLocalSessionAudio("session-1", () => true),
    ).resolves.toBe(false);
    expect(mocks.executeTransaction.mock.calls[0]![0][0].params).toEqual([
      "session-audio:session-1",
      "session-1",
      "absent",
    ]);
  });

  it("revalidates a logical tombstone before retrying file cleanup", async () => {
    await expect(
      cleanupDeletedSessionAudio("session-1", () => true),
    ).resolves.toBe(true);
    expect(mocks.execute).toHaveBeenCalledWith(
      expect.stringMatching(
        /deleted_at IS NOT NULL[\s\S]*attachment_local_state[\s\S]*availability = 'absent'/,
      ),
      ["session-audio:session-1", "session-1"],
    );
    expect(mocks.audioDelete).toHaveBeenCalledWith("session-1");

    vi.clearAllMocks();
    mocks.execute.mockResolvedValue([{ is_deleted: 0 }]);
    await expect(
      cleanupDeletedSessionAudio("session-1", () => true),
    ).resolves.toBe(false);
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });

  it("rechecks capture safety inside the serialized delete operation", async () => {
    await expect(deleteSessionAudio("session-1", () => false)).resolves.toBe(
      false,
    );
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });
});
