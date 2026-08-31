using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace DocumentStudio.G04DC
{
    public sealed class ClassRegistryDigestCollector
    {
        private readonly Stream stream;
        private readonly Encoding encoding;
        private readonly long maximumRawBytes;
        private readonly int maximumRows;
        private readonly int maximumRowCharacters;
        private readonly int maximumCanonicalRowBytes;
        private readonly long maximumCanonicalBytes;
        private readonly int readBufferBytes;
        private readonly List<string> rawRows;
        private long rawByteCount;
        private int observedRowCount;
        private long readElapsedMilliseconds;
        private long normalizationElapsedMilliseconds;
        private long canonicalHashElapsedMilliseconds;
        private long canonicalByteCount;
        private string[] canonicalRows;
        private string sha256;
        private bool sawAnyCharacter;
        private bool endedWithLineFeed;

        public ClassRegistryDigestCollector(
            Stream stream,
            Encoding encoding,
            long maximumRawBytes,
            int maximumRows,
            int maximumRowCharacters,
            int maximumCanonicalRowBytes,
            long maximumCanonicalBytes,
            int readBufferBytes)
        {
            if (stream == null) throw new ArgumentNullException("stream");
            if (encoding == null) throw new ArgumentNullException("encoding");
            if (maximumRawBytes < 1) throw new ArgumentOutOfRangeException("maximumRawBytes");
            if (maximumRows < 1) throw new ArgumentOutOfRangeException("maximumRows");
            if (maximumRowCharacters < 1) throw new ArgumentOutOfRangeException("maximumRowCharacters");
            if (maximumCanonicalRowBytes < 1) throw new ArgumentOutOfRangeException("maximumCanonicalRowBytes");
            if (maximumCanonicalBytes < 1) throw new ArgumentOutOfRangeException("maximumCanonicalBytes");
            if (readBufferBytes < 1 || readBufferBytes > 1048576) throw new ArgumentOutOfRangeException("readBufferBytes");

            this.stream = stream;
            this.encoding = encoding;
            this.maximumRawBytes = maximumRawBytes;
            this.maximumRows = maximumRows;
            this.maximumRowCharacters = maximumRowCharacters;
            this.maximumCanonicalRowBytes = maximumCanonicalRowBytes;
            this.maximumCanonicalBytes = maximumCanonicalBytes;
            this.readBufferBytes = readBufferBytes;
            this.rawRows = new List<string>();
        }

        public Task ReadTask { get; private set; }

        public long RawByteCount
        {
            get { return Interlocked.Read(ref rawByteCount); }
        }

        public int RowCount
        {
            get { return Volatile.Read(ref observedRowCount); }
        }

        public long ReadElapsedMilliseconds
        {
            get { return Interlocked.Read(ref readElapsedMilliseconds); }
        }

        public long NormalizationElapsedMilliseconds
        {
            get { return Interlocked.Read(ref normalizationElapsedMilliseconds); }
        }

        public long CanonicalHashElapsedMilliseconds
        {
            get { return Interlocked.Read(ref canonicalHashElapsedMilliseconds); }
        }

        public long CanonicalByteCount
        {
            get { return Interlocked.Read(ref canonicalByteCount); }
        }

        public string Sha256
        {
            get { return sha256; }
        }

        public Task BeginRead()
        {
            if (ReadTask != null) throw new InvalidOperationException("Registry digest read was already started.");
            ReadTask = Task.Factory.StartNew(
                ReadCore,
                CancellationToken.None,
                TaskCreationOptions.LongRunning,
                TaskScheduler.Default);
            return ReadTask;
        }

        private void ReadCore()
        {
            Stopwatch stopwatch = Stopwatch.StartNew();
            byte[] byteBuffer = new byte[readBufferBytes];
            char[] characterBuffer = new char[encoding.GetMaxCharCount(readBufferBytes)];
            Decoder decoder = encoding.GetDecoder();
            StringBuilder currentRow = new StringBuilder();
            bool firstCharacter = true;
            try
            {
                while (true)
                {
                    int read = stream.Read(byteBuffer, 0, byteBuffer.Length);
                    if (read < 0 || read > byteBuffer.Length)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_READ_INVALID] Native query returned an invalid byte count.");
                    }
                    if (read == 0) break;
                    long totalBytes = Interlocked.Add(ref rawByteCount, read);
                    if (totalBytes > maximumRawBytes)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_RAW_BYTE_CEILING] Native query exceeded the raw-byte ceiling.");
                    }
                    DecodeBytes(decoder, byteBuffer, read, characterBuffer, false, currentRow, ref firstCharacter);
                }

                DecodeBytes(decoder, byteBuffer, 0, characterBuffer, true, currentRow, ref firstCharacter);
                if (currentRow.Length != 0) AddRawRow(currentRow);
            }
            catch (DecoderFallbackException exception)
            {
                throw new InvalidDataException("[REGISTRY_DIGEST_DECODING_INVALID] Native query output is not valid in the explicit encoding.", exception);
            }
            finally
            {
                stopwatch.Stop();
                Interlocked.Exchange(ref readElapsedMilliseconds, stopwatch.ElapsedMilliseconds);
            }
        }

        private void DecodeBytes(
            Decoder decoder,
            byte[] byteBuffer,
            int byteCount,
            char[] characterBuffer,
            bool flush,
            StringBuilder currentRow,
            ref bool firstCharacter)
        {
            int byteIndex = 0;
            bool completed = false;
            while (!completed)
            {
                int bytesUsed;
                int charactersUsed;
                decoder.Convert(
                    byteBuffer,
                    byteIndex,
                    byteCount - byteIndex,
                    characterBuffer,
                    0,
                    characterBuffer.Length,
                    flush,
                    out bytesUsed,
                    out charactersUsed,
                    out completed);
                byteIndex += bytesUsed;
                for (int index = 0; index < charactersUsed; index++)
                {
                    char character = characterBuffer[index];
                    if (firstCharacter)
                    {
                        firstCharacter = false;
                        if (character == '\uFEFF') continue;
                    }
                    sawAnyCharacter = true;
                    endedWithLineFeed = character == '\n';
                    if (character == '\n')
                    {
                        if (currentRow.Length != 0 && currentRow[currentRow.Length - 1] == '\r')
                        {
                            currentRow.Length -= 1;
                        }
                        AddRawRow(currentRow);
                        currentRow.Length = 0;
                    }
                    else
                    {
                        currentRow.Append(character);
                        if (currentRow.Length > maximumRowCharacters)
                        {
                            throw new InvalidDataException("[REGISTRY_DIGEST_ROW_LENGTH_CEILING] Native query row exceeded the character ceiling.");
                        }
                    }
                }
                if (byteCount == 0 && charactersUsed == 0) break;
            }
        }

        private void AddRawRow(StringBuilder row)
        {
            if (rawRows.Count >= maximumRows)
            {
                throw new InvalidDataException("[REGISTRY_DIGEST_ROW_CEILING] Native query exceeded the row ceiling.");
            }
            rawRows.Add(row.ToString());
            Volatile.Write(ref observedRowCount, rawRows.Count);
        }

        public void AppendStderrText(string stderr)
        {
            EnsureReadCompleted();
            if (canonicalRows != null) throw new InvalidOperationException("Registry digest rows were already normalized.");
            if (String.IsNullOrEmpty(stderr)) return;
            if (!sawAnyCharacter || endedWithLineFeed) AddRawRow(new StringBuilder());
            StringBuilder currentRow = new StringBuilder();
            for (int index = 0; index < stderr.Length; index++)
            {
                char character = stderr[index];
                if (character == '\n')
                {
                    if (currentRow.Length != 0 && currentRow[currentRow.Length - 1] == '\r')
                    {
                        currentRow.Length -= 1;
                    }
                    AddRawRow(currentRow);
                    currentRow.Length = 0;
                }
                else
                {
                    currentRow.Append(character);
                    if (currentRow.Length > maximumRowCharacters)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_ROW_LENGTH_CEILING] Native query stderr row exceeded the character ceiling.");
                    }
                }
            }
            if (currentRow.Length != 0) AddRawRow(currentRow);
            sawAnyCharacter = sawAnyCharacter || stderr.Length != 0;
            endedWithLineFeed = stderr.Length != 0 && stderr[stderr.Length - 1] == '\n';
        }

        public void Normalize(long budgetMilliseconds)
        {
            EnsureReadCompleted();
            if (canonicalRows != null) throw new InvalidOperationException("Registry digest rows were already normalized.");
            Stopwatch stopwatch = Stopwatch.StartNew();
            string[] normalized = new string[rawRows.Count];
            long totalBytes = 0;
            for (int index = 0; index < rawRows.Count; index++)
            {
                CheckBudget(stopwatch, budgetMilliseconds, index);
                string canonical = QuoteJsonString(rawRows[index].TrimEnd());
                int rowBytes = encoding.GetByteCount(canonical);
                if (rowBytes > maximumCanonicalRowBytes)
                {
                    throw new InvalidDataException("[REGISTRY_DIGEST_ROW_LENGTH_CEILING] Canonical row exceeded the byte ceiling.");
                }
                totalBytes += rowBytes + 2L;
                if (totalBytes > maximumCanonicalBytes)
                {
                    throw new InvalidDataException("[REGISTRY_DIGEST_CANONICAL_BYTE_CEILING] Canonical rows exceeded the aggregate-byte ceiling.");
                }
                normalized[index] = canonical;
            }
            CheckBudget(stopwatch, budgetMilliseconds, rawRows.Count);
            canonicalRows = normalized;
            rawRows.Clear();
            Interlocked.Exchange(ref canonicalByteCount, totalBytes);
            stopwatch.Stop();
            Interlocked.Exchange(ref normalizationElapsedMilliseconds, stopwatch.ElapsedMilliseconds);
        }

        public string Hash(long budgetMilliseconds)
        {
            if (canonicalRows == null) throw new InvalidOperationException("Registry digest rows were not normalized.");
            if (sha256 != null) throw new InvalidOperationException("Registry digest was already finalized.");
            Stopwatch stopwatch = Stopwatch.StartNew();
            CheckBudget(stopwatch, budgetMilliseconds, 0);
            Array.Sort(canonicalRows, new BudgetedStringComparer(stopwatch, budgetMilliseconds));
            CheckBudget(stopwatch, budgetMilliseconds, 0);
            using (SHA256 hash = SHA256.Create())
            {
                byte[] newline = new byte[] { 10 };
                for (int index = 0; index < canonicalRows.Length; index++)
                {
                    CheckBudget(stopwatch, budgetMilliseconds, index);
                    if (index != 0) hash.TransformBlock(newline, 0, newline.Length, newline, 0);
                    byte[] rowBytes = encoding.GetBytes(canonicalRows[index]);
                    if (rowBytes.Length != 0) hash.TransformBlock(rowBytes, 0, rowBytes.Length, rowBytes, 0);
                }
                hash.TransformFinalBlock(new byte[0], 0, 0);
                sha256 = ToLowerHex(hash.Hash);
            }
            CheckBudget(stopwatch, budgetMilliseconds, canonicalRows.Length);
            stopwatch.Stop();
            Interlocked.Exchange(ref canonicalHashElapsedMilliseconds, stopwatch.ElapsedMilliseconds);
            return sha256;
        }

        private void EnsureReadCompleted()
        {
            if (ReadTask == null || !ReadTask.IsCompleted)
            {
                throw new InvalidOperationException("Registry digest read has not completed.");
            }
            ReadTask.GetAwaiter().GetResult();
        }

        private static void CheckBudget(Stopwatch stopwatch, long budgetMilliseconds, int itemCount)
        {
            if (budgetMilliseconds < 1 || stopwatch.ElapsedMilliseconds > budgetMilliseconds)
            {
                throw new TimeoutException("[REGISTRY_DIGEST_TIMEOUT] Registry digest substage exceeded its remaining phase budget; itemCount=" + itemCount.ToString(CultureInfo.InvariantCulture) + ".");
            }
        }

        private sealed class BudgetedStringComparer : IComparer<string>
        {
            private readonly Stopwatch stopwatch;
            private readonly long budgetMilliseconds;
            private int comparisonCount;

            public BudgetedStringComparer(Stopwatch stopwatch, long budgetMilliseconds)
            {
                this.stopwatch = stopwatch;
                this.budgetMilliseconds = budgetMilliseconds;
            }

            public int Compare(string left, string right)
            {
                comparisonCount++;
                if ((comparisonCount & 1023) == 0)
                {
                    CheckBudget(stopwatch, budgetMilliseconds, comparisonCount);
                }
                return StringComparer.CurrentCultureIgnoreCase.Compare(left, right);
            }
        }

        private static string QuoteJsonString(string value)
        {
            StringBuilder builder = new StringBuilder(value.Length + 2);
            builder.Append('"');
            for (int index = 0; index < value.Length; index++)
            {
                char character = value[index];
                switch (character)
                {
                    case '"': builder.Append("\\\""); break;
                    case '\\': builder.Append("\\\\"); break;
                    case '\b': builder.Append("\\b"); break;
                    case '\t': builder.Append("\\t"); break;
                    case '\n': builder.Append("\\n"); break;
                    case '\f': builder.Append("\\f"); break;
                    case '\r': builder.Append("\\r"); break;
                    default:
                        if (character < 0x20 || character == 0x85 || character == 0x2028 || character == 0x2029)
                        {
                            builder.Append("\\u");
                            builder.Append(((int)character).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else if (char.IsSurrogate(character))
                        {
                            if (char.IsHighSurrogate(character) && index + 1 < value.Length && char.IsLowSurrogate(value[index + 1]))
                            {
                                builder.Append(character);
                                builder.Append(value[++index]);
                            }
                            else
                            {
                                builder.Append('\uFFFD');
                            }
                        }
                        else
                        {
                            builder.Append(character);
                        }
                        break;
                }
            }
            builder.Append('"');
            return builder.ToString();
        }

        private static string ToLowerHex(byte[] bytes)
        {
            StringBuilder builder = new StringBuilder(bytes.Length * 2);
            for (int index = 0; index < bytes.Length; index++)
            {
                builder.Append(bytes[index].ToString("x2", CultureInfo.InvariantCulture));
            }
            return builder.ToString();
        }
    }

    public sealed class BoundedTextCapture
    {
        private readonly Stream stream;
        private readonly Encoding encoding;
        private readonly long maximumBytes;
        private readonly int readBufferBytes;
        private long rawByteCount;
        private string text;

        public BoundedTextCapture(Stream stream, Encoding encoding, long maximumBytes, int readBufferBytes)
        {
            if (stream == null) throw new ArgumentNullException("stream");
            if (encoding == null) throw new ArgumentNullException("encoding");
            if (maximumBytes < 1) throw new ArgumentOutOfRangeException("maximumBytes");
            if (readBufferBytes < 1 || readBufferBytes > 1048576) throw new ArgumentOutOfRangeException("readBufferBytes");
            this.stream = stream;
            this.encoding = encoding;
            this.maximumBytes = maximumBytes;
            this.readBufferBytes = readBufferBytes;
        }

        public Task ReadTask { get; private set; }

        public long RawByteCount
        {
            get { return Interlocked.Read(ref rawByteCount); }
        }

        public string Text
        {
            get
            {
                if (ReadTask == null || !ReadTask.IsCompleted) throw new InvalidOperationException("Bounded text read has not completed.");
                ReadTask.GetAwaiter().GetResult();
                return text;
            }
        }

        public Task BeginRead()
        {
            if (ReadTask != null) throw new InvalidOperationException("Bounded text read was already started.");
            ReadTask = Task.Factory.StartNew(
                ReadCore,
                CancellationToken.None,
                TaskCreationOptions.LongRunning,
                TaskScheduler.Default);
            return ReadTask;
        }

        private void ReadCore()
        {
            byte[] buffer = new byte[readBufferBytes];
            MemoryStream captured = new MemoryStream();
            try
            {
                while (true)
                {
                    int read = stream.Read(buffer, 0, buffer.Length);
                    if (read < 0 || read > buffer.Length)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_STDERR_INVALID] Native query stderr returned an invalid byte count.");
                    }
                    if (read == 0) break;
                    long total = Interlocked.Add(ref rawByteCount, read);
                    if (total > maximumBytes)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_STDERR_CEILING] Native query stderr exceeded the byte ceiling.");
                    }
                    captured.Write(buffer, 0, read);
                }
                text = encoding.GetString(captured.ToArray()).TrimStart('\uFEFF');
            }
            catch (DecoderFallbackException exception)
            {
                throw new InvalidDataException("[REGISTRY_DIGEST_DECODING_INVALID] Native query stderr is not valid in the explicit encoding.", exception);
            }
            finally
            {
                captured.Dispose();
            }
        }
    }
}
