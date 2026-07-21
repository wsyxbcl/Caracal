const XML_HEADER = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
const XLSX_MIME =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const TEXT_ENCODER = new TextEncoder();

const TRAP_INFO_HEADERS = [
    ["相机位点名称", "deploymentName"],
    ["监测员", "setupBy"],
    ["纬度", "latitude"],
    ["经度", "longitude"],
    ["位点来源", "locationSource"],
    ["相机工作日时长", "cameraWorkingDays"],
    ["开始时间", "deploymentStart"],
    ["结束时间", "deploymentEnd"],
    ["备注", "deploymentComments"],
    ["照片时间是否有问题", "timestampProblem"],
    ["参考年份", "timestampRef"],
    ["其它问题1", "otherProblem1"],
    ["其它问题1开始时间", "otherProblem1Start"],
    ["其它问题1结束时间", "otherProblem1End"],
    ["时间跳错1开始时间", "exifTimeProblem1Start"],
    ["时间跳错1结束时间", "exifTimeProblem1End"],
    ["时间跳错1是否经过校正", "exifTimeProblem1Revised"],
    ["时间跳错1参考年份", "exifTimeProblem1Ref"],
    ["其它问题2", "otherProblem2"],
    ["其它问题2开始时间", "otherProblem2Start"],
    ["其它问题2结束时间", "otherProblem2End"],
    ["时间跳错2开始时间", "exifTimeProblem2Start"],
    ["时间跳错2结束时间", "exifTimeProblem2End"],
    ["时间跳错2是否经过校正", "exifTimeProblem2Revised"],
    ["时间跳错2参考年份", "exifTimeProblem2Ref"],
];

const TRAP_INFO_VALIDATIONS = [
    { field: "locationSource", options: ["估计GPS", "精确GPS"] },
    {
        field: "timestampProblem",
        options: ["有问题", "无问题", "有问题但已精确校正"],
    },
    {
        field: "exifTimeProblem1Revised",
        options: ["已校正", "未校正"],
    },
    {
        field: "exifTimeProblem2Revised",
        options: ["已校正", "未校正"],
    },
    {
        field: "otherProblem1",
        options: ["照片损坏", "未有效工作", "其它(在备注中注明)"],
    },
    {
        field: "otherProblem2",
        options: ["照片损坏", "未有效工作", "其它(在备注中注明)"],
    },
];

const FIELD_TO_COLUMN = Object.fromEntries(
    TRAP_INFO_HEADERS.map(([, field], index) => [field, index + 1]),
);

// Default 备注 (deploymentComments) value for deployments whose media metadata
// carried GPS coordinates.
const GPS_COMMENT = "元数据含GPS坐标";

const CRC32_TABLE = buildCrc32Table();

export function sortDeploymentOptions(deploymentOptions) {
    return [...(deploymentOptions || [])].sort((left, right) =>
        left.deployment.localeCompare(right.deployment, undefined, {
            numeric: true,
            sensitivity: "base",
        }),
    );
}

export function buildTrapInfoWorkbookBytes(deploymentOptions) {
    const rows = sortDeploymentOptions(deploymentOptions).map((option) => ({
        deployment: option.deployment || "",
        deploymentStart: option.first_seen || "",
        deploymentEnd: option.last_seen || "",
        deploymentComments: option.has_gps ? GPS_COMMENT : "",
    }));

    const timestamp = new Date().toISOString();
    const entries = [
        {
            name: "[Content_Types].xml",
            data: buildContentTypesXml(),
        },
        {
            name: "_rels/.rels",
            data: buildRootRelationshipsXml(),
        },
        {
            name: "docProps/app.xml",
            data: buildAppPropertiesXml(),
        },
        {
            name: "docProps/core.xml",
            data: buildCorePropertiesXml(timestamp),
        },
        {
            name: "xl/workbook.xml",
            data: buildWorkbookXml(),
        },
        {
            name: "xl/_rels/workbook.xml.rels",
            data: buildWorkbookRelationshipsXml(),
        },
        {
            name: "xl/styles.xml",
            data: buildStylesXml(),
        },
        {
            name: "xl/worksheets/sheet1.xml",
            data: buildWorksheetXml(rows),
        },
    ];

    return createZip(entries);
}

export function downloadTrapInfoTemplate(
    deploymentOptions,
    filename = "trap_info_template.xlsx",
) {
    const workbookBytes = buildTrapInfoWorkbookBytes(deploymentOptions);
    const blob = new Blob([workbookBytes], { type: XLSX_MIME });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = filename;
    link.style.display = "none";
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function buildContentTypesXml() {
    return (
        XML_HEADER +
        `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
        `<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>` +
        `<Default Extension="xml" ContentType="application/xml"/>` +
        `<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>` +
        `<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>` +
        `<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>` +
        `<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>` +
        `<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>` +
        `</Types>`
    );
}

function buildRootRelationshipsXml() {
    return (
        XML_HEADER +
        `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
        `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>` +
        `<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>` +
        `<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>` +
        `</Relationships>`
    );
}

function buildAppPropertiesXml() {
    return (
        XML_HEADER +
        `<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" ` +
        `xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">` +
        `<Application>serval-charton</Application>` +
        `<DocSecurity>0</DocSecurity>` +
        `<ScaleCrop>false</ScaleCrop>` +
        `<HeadingPairs><vt:vector size="2" baseType="variant">` +
        `<vt:variant><vt:lpstr>Worksheets</vt:lpstr></vt:variant>` +
        `<vt:variant><vt:i4>1</vt:i4></vt:variant>` +
        `</vt:vector></HeadingPairs>` +
        `<TitlesOfParts><vt:vector size="1" baseType="lpstr">` +
        `<vt:lpstr>trap_info</vt:lpstr>` +
        `</vt:vector></TitlesOfParts>` +
        `<Company></Company>` +
        `<LinksUpToDate>false</LinksUpToDate>` +
        `<SharedDoc>false</SharedDoc>` +
        `<HyperlinksChanged>false</HyperlinksChanged>` +
        `<AppVersion>1.0</AppVersion>` +
        `</Properties>`
    );
}

function buildCorePropertiesXml(timestamp) {
    const escapedTimestamp = escapeXml(timestamp);
    return (
        XML_HEADER +
        `<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" ` +
        `xmlns:dc="http://purl.org/dc/elements/1.1/" ` +
        `xmlns:dcterms="http://purl.org/dc/terms/" ` +
        `xmlns:dcmitype="http://purl.org/dc/dcmitype/" ` +
        `xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">` +
        `<dc:creator>serval-charton</dc:creator>` +
        `<cp:lastModifiedBy>serval-charton</cp:lastModifiedBy>` +
        `<dcterms:created xsi:type="dcterms:W3CDTF">${escapedTimestamp}</dcterms:created>` +
        `<dcterms:modified xsi:type="dcterms:W3CDTF">${escapedTimestamp}</dcterms:modified>` +
        `</cp:coreProperties>`
    );
}

function buildWorkbookXml() {
    return (
        XML_HEADER +
        `<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" ` +
        `xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">` +
        `<sheets><sheet name="trap_info" sheetId="1" r:id="rId1"/></sheets>` +
        `</workbook>`
    );
}

function buildWorkbookRelationshipsXml() {
    return (
        XML_HEADER +
        `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
        `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>` +
        `<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>` +
        `</Relationships>`
    );
}

function buildStylesXml() {
    return (
        XML_HEADER +
        `<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">` +
        `<fonts count="1"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font></fonts>` +
        `<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>` +
        `<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>` +
        `<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>` +
        `<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>` +
        `<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>` +
        `</styleSheet>`
    );
}

function buildWorksheetXml(rows) {
    const lastRow = Math.max(rows.length + 2, 2);
    const lastColumn = columnName(TRAP_INFO_HEADERS.length);
    const columnsXml = TRAP_INFO_HEADERS.map((_, index) => {
        const col = index + 1;
        return `<col min="${col}" max="${col}" width="20" customWidth="1"/>`;
    }).join("");

    const chineseHeaderRow = buildHeaderRow(
        1,
        TRAP_INFO_HEADERS.map(([label]) => label),
    );
    const englishHeaderRow = buildHeaderRow(
        2,
        TRAP_INFO_HEADERS.map(([, key]) => key),
    );
    const dataRows = rows
        .map((row, index) => {
            const rowNumber = index + 3;
            const commentsColumn = columnName(FIELD_TO_COLUMN.deploymentComments);
            const commentsCell = row.deploymentComments
                ? inlineStringCell(
                      `${commentsColumn}${rowNumber}`,
                      row.deploymentComments,
                  )
                : "";
            return (
                `<row r="${rowNumber}">` +
                inlineStringCell(`A${rowNumber}`, row.deployment) +
                inlineStringCell(`G${rowNumber}`, row.deploymentStart) +
                inlineStringCell(`H${rowNumber}`, row.deploymentEnd) +
                commentsCell +
                `</row>`
            );
        })
        .join("");

    const validationsXml =
        rows.length > 0
            ? `<dataValidations count="${TRAP_INFO_VALIDATIONS.length}">` +
              TRAP_INFO_VALIDATIONS.map((validation) => {
                  const column = columnName(FIELD_TO_COLUMN[validation.field]);
                  const formula = `"${validation.options.join(",")}"`;
                  return (
                      `<dataValidation type="list" allowBlank="1" showInputMessage="1" showErrorMessage="1" sqref="${column}3:${column}${rows.length + 2}">` +
                      `<formula1>${escapeXml(formula)}</formula1>` +
                      `</dataValidation>`
                  );
              }).join("") +
              `</dataValidations>`
            : "";

    return (
        XML_HEADER +
        `<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">` +
        `<dimension ref="A1:${lastColumn}${lastRow}"/>` +
        `<sheetViews><sheetView workbookViewId="0"/></sheetViews>` +
        `<sheetFormatPr defaultRowHeight="15"/>` +
        `<cols>${columnsXml}</cols>` +
        `<sheetData>${chineseHeaderRow}${englishHeaderRow}${dataRows}</sheetData>` +
        validationsXml +
        `<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>` +
        `</worksheet>`
    );
}

function buildHeaderRow(rowNumber, values) {
    return (
        `<row r="${rowNumber}">` +
        values
            .map((value, index) =>
                inlineStringCell(`${columnName(index + 1)}${rowNumber}`, value),
            )
            .join("") +
        `</row>`
    );
}

function inlineStringCell(reference, value) {
    return `<c r="${reference}" t="inlineStr"><is><t>${escapeXml(value || "")}</t></is></c>`;
}

function columnName(index) {
    let name = "";
    let current = index;
    while (current > 0) {
        current -= 1;
        name = String.fromCharCode(65 + (current % 26)) + name;
        current = Math.floor(current / 26);
    }
    return name;
}

function escapeXml(value) {
    return String(value)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&apos;");
}

function buildCrc32Table() {
    const table = new Uint32Array(256);
    for (let index = 0; index < 256; index += 1) {
        let value = index;
        for (let bit = 0; bit < 8; bit += 1) {
            value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
        }
        table[index] = value >>> 0;
    }
    return table;
}

function crc32(bytes) {
    let value = 0xffffffff;
    for (const byte of bytes) {
        value = CRC32_TABLE[(value ^ byte) & 0xff] ^ (value >>> 8);
    }
    return (value ^ 0xffffffff) >>> 0;
}

function createZip(entries) {
    const localChunks = [];
    const centralChunks = [];
    let offset = 0;

    for (const entry of entries) {
        const nameBytes = TEXT_ENCODER.encode(entry.name);
        const dataBytes =
            entry.data instanceof Uint8Array
                ? entry.data
                : TEXT_ENCODER.encode(entry.data);
        const crc = crc32(dataBytes);
        const localHeader = new Uint8Array(30 + nameBytes.length);
        const centralHeader = new Uint8Array(46 + nameBytes.length);

        writeUint32LE(localHeader, 0, 0x04034b50);
        writeUint16LE(localHeader, 4, 20);
        writeUint16LE(localHeader, 6, 0x0800);
        writeUint16LE(localHeader, 8, 0);
        writeUint16LE(localHeader, 10, 0);
        writeUint16LE(localHeader, 12, 0);
        writeUint32LE(localHeader, 14, crc);
        writeUint32LE(localHeader, 18, dataBytes.length);
        writeUint32LE(localHeader, 22, dataBytes.length);
        writeUint16LE(localHeader, 26, nameBytes.length);
        writeUint16LE(localHeader, 28, 0);
        localHeader.set(nameBytes, 30);

        writeUint32LE(centralHeader, 0, 0x02014b50);
        writeUint16LE(centralHeader, 4, 20);
        writeUint16LE(centralHeader, 6, 20);
        writeUint16LE(centralHeader, 8, 0x0800);
        writeUint16LE(centralHeader, 10, 0);
        writeUint16LE(centralHeader, 12, 0);
        writeUint16LE(centralHeader, 14, 0);
        writeUint32LE(centralHeader, 16, crc);
        writeUint32LE(centralHeader, 20, dataBytes.length);
        writeUint32LE(centralHeader, 24, dataBytes.length);
        writeUint16LE(centralHeader, 28, nameBytes.length);
        writeUint16LE(centralHeader, 30, 0);
        writeUint16LE(centralHeader, 32, 0);
        writeUint16LE(centralHeader, 34, 0);
        writeUint16LE(centralHeader, 36, 0);
        writeUint32LE(centralHeader, 38, 0);
        writeUint32LE(centralHeader, 42, offset);
        centralHeader.set(nameBytes, 46);

        localChunks.push(localHeader, dataBytes);
        centralChunks.push(centralHeader);
        offset += localHeader.length + dataBytes.length;
    }

    const centralDirectory = concatBytes(centralChunks);
    const localSection = concatBytes(localChunks);
    const endOfCentralDirectory = new Uint8Array(22);
    writeUint32LE(endOfCentralDirectory, 0, 0x06054b50);
    writeUint16LE(endOfCentralDirectory, 4, 0);
    writeUint16LE(endOfCentralDirectory, 6, 0);
    writeUint16LE(endOfCentralDirectory, 8, entries.length);
    writeUint16LE(endOfCentralDirectory, 10, entries.length);
    writeUint32LE(endOfCentralDirectory, 12, centralDirectory.length);
    writeUint32LE(endOfCentralDirectory, 16, localSection.length);
    writeUint16LE(endOfCentralDirectory, 20, 0);

    return concatBytes([localSection, centralDirectory, endOfCentralDirectory]);
}

function concatBytes(chunks) {
    const length = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
    const merged = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
        merged.set(chunk, offset);
        offset += chunk.length;
    }
    return merged;
}

function writeUint16LE(buffer, offset, value) {
    buffer[offset] = value & 0xff;
    buffer[offset + 1] = (value >>> 8) & 0xff;
}

function writeUint32LE(buffer, offset, value) {
    buffer[offset] = value & 0xff;
    buffer[offset + 1] = (value >>> 8) & 0xff;
    buffer[offset + 2] = (value >>> 16) & 0xff;
    buffer[offset + 3] = (value >>> 24) & 0xff;
}
