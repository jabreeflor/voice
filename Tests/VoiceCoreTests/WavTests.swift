import XCTest
@testable import VoiceCore

/// `Recorder.wavData` hand-builds the RIFF container that whisper.cpp is fed.
/// A wrong byte in the 44-byte header means silent mis-transcription rather
/// than a crash, so every field is pinned here.
final class WavTests: XCTestCase {

    // MARK: - Helpers

    private func ascii(_ data: Data, _ range: Range<Int>) -> String {
        String(decoding: data[range], as: UTF8.self)
    }

    private func u32(_ data: Data, at offset: Int) -> UInt32 {
        var v: UInt32 = 0
        for i in (0..<4).reversed() { v = v << 8 | UInt32(data[offset + i]) }
        return v
    }

    private func u16(_ data: Data, at offset: Int) -> UInt16 {
        UInt16(data[offset]) | UInt16(data[offset + 1]) << 8
    }

    private func i16(_ data: Data, at offset: Int) -> Int16 {
        Int16(bitPattern: u16(data, at: offset))
    }

    // MARK: - Chunk identifiers

    func testRiffWaveChunkIdentifiers() {
        let d = Recorder.wavData(samples: [0, 0, 0, 0])
        XCTAssertEqual(ascii(d, 0..<4), "RIFF")
        XCTAssertEqual(ascii(d, 8..<12), "WAVE")
        XCTAssertEqual(ascii(d, 12..<16), "fmt ")
        XCTAssertEqual(ascii(d, 36..<40), "data")
    }

    // MARK: - fmt chunk

    func testFormatChunkDescribes16kHzMono16BitPCM() {
        let d = Recorder.wavData(samples: [0])
        XCTAssertEqual(u32(d, at: 16), 16, "fmt chunk size should be 16 (PCM)")
        XCTAssertEqual(u16(d, at: 20), 1, "audio format should be 1 (uncompressed PCM)")
        XCTAssertEqual(u16(d, at: 22), 1, "should be mono")
        XCTAssertEqual(u32(d, at: 24), 16000, "sample rate should be 16 kHz")
        XCTAssertEqual(u32(d, at: 28), 32000, "byte rate = 16000 * 1 channel * 2 bytes")
        XCTAssertEqual(u16(d, at: 32), 2, "block align = 1 channel * 2 bytes")
        XCTAssertEqual(u16(d, at: 34), 16, "should be 16-bit samples")
    }

    /// Header fields are fixed, not derived from the sample array, so they must
    /// hold for any length.
    func testFormatChunkIsIdenticalRegardlessOfSampleCount() {
        let short = Recorder.wavData(samples: [Float](repeating: 0.1, count: 3))
        let long = Recorder.wavData(samples: [Float](repeating: 0.1, count: 16000))
        XCTAssertEqual(short[12..<36], long[12..<36])
    }

    // MARK: - Sizes

    func testDataSizeIsTwoBytesPerSample() {
        for count in [0, 1, 7, 1000, 16000] {
            let d = Recorder.wavData(samples: [Float](repeating: 0, count: count))
            XCTAssertEqual(u32(d, at: 40), UInt32(count * 2), "data size wrong for \(count) samples")
        }
    }

    func testRiffSizeIsDataSizePlus36() {
        for count in [0, 1, 7, 1000, 16000] {
            let d = Recorder.wavData(samples: [Float](repeating: 0, count: count))
            XCTAssertEqual(u32(d, at: 4), UInt32(36 + count * 2), "RIFF size wrong for \(count) samples")
        }
    }

    func testTotalLengthIsHeaderPlusPayload() {
        for count in [0, 1, 7, 1000, 16000] {
            let d = Recorder.wavData(samples: [Float](repeating: 0, count: count))
            XCTAssertEqual(d.count, 44 + count * 2, "total length wrong for \(count) samples")
        }
    }

    /// The declared sizes must agree with the bytes actually present, or the
    /// decoder reads past the end of the buffer.
    func testDeclaredSizesAgreeWithActualByteCount() {
        let d = Recorder.wavData(samples: [Float](repeating: 0.25, count: 512))
        XCTAssertEqual(Int(u32(d, at: 40)), d.count - 44)
        XCTAssertEqual(Int(u32(d, at: 4)), d.count - 8)
    }

    // MARK: - Empty input

    func testEmptyInputProducesHeaderOnlyFile() {
        let d = Recorder.wavData(samples: [])
        XCTAssertEqual(d.count, 44)
        XCTAssertEqual(ascii(d, 0..<4), "RIFF")
        XCTAssertEqual(ascii(d, 8..<12), "WAVE")
        XCTAssertEqual(u32(d, at: 4), 36)
        XCTAssertEqual(u32(d, at: 40), 0)
    }

    // MARK: - Sample conversion

    func testSilenceEncodesAsZeroSamples() {
        let d = Recorder.wavData(samples: [0, 0, 0])
        for i in 0..<3 {
            XCTAssertEqual(i16(d, at: 44 + i * 2), 0)
        }
    }

    func testFullScaleSamplesMapToInt16Extremes() {
        let d = Recorder.wavData(samples: [1.0, -1.0])
        XCTAssertEqual(i16(d, at: 44), 32767)
        XCTAssertEqual(i16(d, at: 46), -32767)
    }

    /// Out-of-range floats are clamped rather than wrapping around, which would
    /// turn a loud passage into audible garbage.
    func testOutOfRangeSamplesAreClampedNotWrapped() {
        let d = Recorder.wavData(samples: [4.5, -4.5, 100, -100])
        XCTAssertEqual(i16(d, at: 44), 32767)
        XCTAssertEqual(i16(d, at: 46), -32767)
        XCTAssertEqual(i16(d, at: 48), 32767)
        XCTAssertEqual(i16(d, at: 50), -32767)
    }

    func testMidScaleSampleIsScaledBy32767() {
        let d = Recorder.wavData(samples: [0.5, -0.5])
        XCTAssertEqual(i16(d, at: 44), Int16(0.5 as Float * 32767))
        XCTAssertEqual(i16(d, at: 46), Int16(-0.5 as Float * 32767))
    }

    func testSamplesAreWrittenInOrderLittleEndian() {
        let d = Recorder.wavData(samples: [0, 1.0, 0, -1.0])
        XCTAssertEqual(i16(d, at: 44), 0)
        XCTAssertEqual(i16(d, at: 46), 32767)
        XCTAssertEqual(i16(d, at: 48), 0)
        XCTAssertEqual(i16(d, at: 50), -32767)
        // 32767 little-endian is 0xFF 0x7F.
        XCTAssertEqual(d[46], 0xFF)
        XCTAssertEqual(d[47], 0x7F)
    }

    // MARK: - Shape of a realistic buffer

    /// The engine warm-up pushes half a second of silence through this path.
    func testHalfSecondOfSilenceHasExpectedSize() {
        let d = Recorder.wavData(samples: [Float](repeating: 0, count: 8000))
        XCTAssertEqual(d.count, 44 + 16000)
        XCTAssertEqual(u32(d, at: 24), 16000)
        XCTAssertEqual(Int(u32(d, at: 40)) / 2, 8000, "8000 frames = 0.5s at 16 kHz")
    }
}
