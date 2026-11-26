pub(super) fn check(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        bytes = match bytes {
            [0..=0x7f, rest @ ..] => rest,
            [row, column, rest @ ..] if plane_one(*row, *column) => rest,
            [0x8e, 0xa1, row, column, rest @ ..] if plane_one(*row, *column) => rest,
            [0x8e, 0xa2, row, column @ 0xa1..=0xfe, rest @ ..]
                if matches!(row, 0xa1..=0xf1) || *row == 0xf2 && *column <= 0xc4 =>
            {
                rest
            }
            [
                0x8e,
                0xae,
                row @ 0xa1..=0xe7,
                column @ 0xa1..=0xfe,
                rest @ ..,
            ] if PLANE_FOURTEEN[usize::from(*row - 0xa1)] & (1 << (*column - 0xa1)) != 0 => rest,
            _ => return false,
        };
    }
    true
}

fn plane_one(row: u8, column: u8) -> bool {
    match row {
        0xa1 => matches!(column, 0xa1..=0xb9 | 0xbe..=0xfe),
        0xa2 => matches!(column, 0xa1..=0xa3 | 0xa5 | 0xa7..=0xfe),
        0xa3 => matches!(column, 0xa1..=0xce),
        0xa4 => matches!(column, 0xa1..=0xbd | 0xbf | 0xc1..=0xfe),
        0xa5 => matches!(column, 0xa1..=0xf0),
        0xa6 => matches!(column, 0xa1..=0xbe),
        0xc2 => matches!(column, 0xa1..=0xc1),
        0xc4..=0xfc => matches!(column, 0xa1..=0xfe),
        0xfd => matches!(column, 0xa1..=0xcb),
        _ => false,
    }
}

// Assigned CNS 11643 plane 14 positions, from PHP 8.5.12 mb_check_encoding.
// Row 0 is 0xA1. Bit 0 is column 0xA1. Each row covers 94 columns.
const PLANE_FOURTEEN: [u128; 71] = [
    0x17c2_9dfd_af7f_3ff7_ff7f_ffff,
    0x3afe_fcff_3fcf_bbc0_75f9_77ee,
    0x3bd6_fd63_0a1b_945f_e75d_56fa,
    0x3dfc_9077_ff4d_edbe_deb4_5cf5,
    0x1efb_74e7_7ab5_2dff_a8f7_dfef,
    0x2cec_27ee_ed77_bbdb_11ff_6397,
    0x1ddf_ef77_f713_5fce_ff1d_d5bf,
    0x22f1_b5d7_f8d9_beb1_d0d4_73bf,
    0x097b_df5c_13bf_6bd7_ad7f_e3b3,
    0x3ddf_8ca0_fdea_67ca_76f6_5ee7,
    0x0b43_bff5_3ddd_f7e4_f57e_7dff,
    0x3b6e_fc05_de68_9cbf_7c5a_7ffd,
    0x2fdd_959e_7ad5_f7f2_1bef_f65f,
    0x2fea_67bd_b00f_fdcf_b776_46fb,
    0x27ec_7abf_e7fd_fdfe_acdf_ab6c,
    0x27af_efef_67f2_7690_acfe_c6d3,
    0x1fe6_7aff_f7ec_ddbe_9df6_e9f7,
    0x0dee_d9de_6e53_f0e1_f9a1_dff3,
    0x0e7f_ff06_43d2_7fce_7c43_be6c,
    0x1dfb_65eb_65db_fe7f_d92f_f370,
    0x192b_6287_87db_66ed_4fdf_e7ca,
    0x1b6a_ce3a_ee7f_bcfb_5beb_356e,
    0x37e7_8f77_d157_9378_9f17_f9fd,
    0x277f_fe2d_f4db_f1d3_3d61_256b,
    0x3333_eed1_29fb_6e41_f3d7_bb16,
    0x2fd6_0427_afee_af5e_8cad_67c5,
    0x393f_d0ef_33b9_d45a_f66d_36f8,
    0x1f4f_7dae_9eab_b55b_bdcf_433c,
    0x25f3_81be_7b67_585e_3ff6_77ff,
    0x389a_d387_7fe2_f5df_bcdc_6faf,
    0x2c17_89b6_b179_dd78_e7cc_df0f,
    0x1bbf_0269_ffeb_2ff9_fb66_440b,
    0x3dfe_5690_1521_b59a_32ae_2fcf,
    0x3cdd_f6eb_5dc7_6978_f725_b3a3,
    0x2c76_23ee_a899_5b69_11f9_b7f1,
    0x3fe4_055d_9db8_71b8_efb7_fae6,
    0x2239_bfef_4dca_c562_8bff_f7dc,
    0x26bf_7e8d_9b2c_cabf_fafd_abb5,
    0x3fd2_feff_7e6d_38fe_3f5c_83bf,
    0x01cd_b583_bafa_d4d8_fdbd_a5d7,
    0x1f1f_94be_e9eb_6f1f_7fb6_3676,
    0x0097_ea2f_b43a_2978_31df_de6b,
    0x2b7e_9ff4_7ead_dd7d_fddf_fd42,
    0x37dd_0e7b_e56b_d36d_f7db_f99d,
    0x38d7_f014_9e75_dfa8_1731_fb9a,
    0x323e_eb7b_d10a_7c2a_9ffb_edda,
    0x3d6e_357e_8637_bd9f_cd7f_cc36,
    0x3370_deff_2eed_ff80_d4fd_ee5a,
    0x052f_be66_abb8_9039_ae83_6dee,
    0x2c54_27fe_77ba_c34c_c54c_ebeb,
    0x1b99_8ebf_5f95_998a_bff3_5c7b,
    0x0eb3_dd49_7d5f_eddb_bf76_1b15,
    0x3319_f78b_8dfe_5507_a971_d54d,
    0x0619_d6ff_ff1b_53a2_d728_9d8f,
    0x37f8_be1f_2fd2_929e_ee4c_82a0,
    0x3ffc_6c93_1f99_a91e_efbe_f7e7,
    0x373d_5677_0da1_c73e_bef3_26ad,
    0x3fda_6077_fefe_70fe_4f99_77ba,
    0x3359_7d11_f004_f2df_5fbb_4dd2,
    0x2ce3_d83a_f9f5_4147_7f2a_b37e,
    0x033f_e3ef_a2a5_e97f_7ace_af9f,
    0x2fff_92f7_1ef7_8dde_7e5d_53bb,
    0x2587_2f0b_46de_67b7_ea73_0adb,
    0x246d_58b7_b97e_47f4_fd62_cd16,
    0x3d5e_5d19_d500_b76f_e41e_6bb2,
    0x0103_8b6d_ea41_0313_46b5_c1b7,
    0x0418_2864_002a_1a34_1168_b014,
    0x3fff_ffff_ffff_f780_0007_5282,
    0x3fff_ffff_ffff_ffff_ffff_ffff,
    0x0ff7_ffff_ffdf_ffff_ffff_dfff,
    0x0000_0000_0000_0000_0000_03ff,
];
