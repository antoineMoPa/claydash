/// SDF types definition

/// No more SDF to process
pub const TYPE_END: i32 = 0;
pub const TYPE_SPHERE: i32 = 1;
pub const TYPE_BOX: i32 = 2;


pub const OPERATION_UNION: i32 = 0;
pub const OPERATION_EXCLUSION: i32 = 1;
pub const OPERATION_INTERSECTION: i32 = 2;
pub const OPERATION_USE_LHS_AS_IS: i32 = 3;
pub const OPERATION_USE_RHS_AS_IS: i32 = 4;
pub const OPERATION_END: i32 = 5;
