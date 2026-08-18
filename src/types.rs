#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    pub size: Option<u64>,
    #[serde(rename = "createdDateTime")]
    pub created_date_time: Option<String>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified_date_time: Option<String>,
    #[serde(rename = "webUrl")]
    pub web_url: Option<String>,
    pub folder: Option<FolderFacet>,
    pub file: Option<FileFacet>,
    #[serde(rename = "parentReference")]
    pub parent_reference: Option<ParentReference>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    pub download_url: Option<String>,
    pub description: Option<String>,
}

impl DriveItem {
    pub fn is_dir(&self) -> bool {
        self.folder.is_some()
    }

    pub fn is_file(&self) -> bool {
        self.file.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderFacet {
    #[serde(rename = "childCount")]
    pub child_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFacet {
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    pub hashes: Option<Hashes>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hashes {
    #[serde(rename = "sha1Hash")]
    pub sha1_hash: Option<String>,
    #[serde(rename = "quickXorHash")]
    pub quick_xor_hash: Option<String>,
    #[serde(rename = "crc32Hash")]
    pub crc32_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentReference {
    #[serde(rename = "driveId")]
    pub drive_id: Option<String>,
    pub id: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drive {
    pub id: String,
    #[serde(rename = "driveType")]
    pub drive_type: Option<String>,
    pub owner: Option<IdentitySet>,
    pub quota: Option<Quota>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quota {
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub remaining: Option<u64>,
    pub deleted: Option<u64>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySet {
    pub user: Option<Identity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub id: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveItemList {
    pub value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadSession {
    #[serde(rename = "uploadUrl")]
    pub upload_url: String,
    #[serde(rename = "expirationDateTime")]
    pub expiration_date_time: Option<String>,
    #[serde(rename = "nextExpectedRanges")]
    pub next_expected_ranges: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUploadSessionRequest {
    pub item: CreateUploadSessionItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUploadSessionItem {
    #[serde(
        rename = "@microsoft.graph.conflictBehavior",
        skip_serializing_if = "Option::is_none"
    )]
    pub conflict_behavior: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    pub name: String,
    pub folder: serde_json::Value,
    #[serde(
        rename = "@microsoft.graph.conflictBehavior",
        skip_serializing_if = "Option::is_none"
    )]
    pub conflict_behavior: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveOrRenameRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "parentReference", skip_serializing_if = "Option::is_none")]
    pub parent_reference: Option<ItemReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyItemRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "parentReference", skip_serializing_if = "Option::is_none")]
    pub parent_reference: Option<ItemReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkRequest {
    #[serde(rename = "type")]
    pub link_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: Option<String>,
    pub roles: Option<Vec<String>>,
    pub link: Option<SharingLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharingLink {
    #[serde(rename = "type")]
    pub link_type: Option<String>,
    #[serde(rename = "webUrl")]
    pub web_url: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphErrorResponse {
    pub error: GraphErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "userPrincipalName")]
    pub user_principal_name: Option<String>,
    pub mail: Option<String>,
    pub id: Option<String>,
}
