// Phone format data from FakerPHP 1.24.1.
//
// Copyright (c) 2011 François Zaninotto
// Portions Copyright (c) 2008 Caius Durling
// Portions Copyright (c) 2008 Adam Royle
// Portions Copyright (c) 2008 Fiona Burrows
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

pub(super) const INTERNATIONAL: &[&str] = &[
    "+1##########",
    "+7##########",
    "+20##########",
    "+27##########",
    "+30##########",
    "+31##########",
    "+32#########",
    "+33#########",
    "+34#########",
    "+36#########",
    "+39##########",
    "+40#########",
    "+41#########",
    "+43##########",
    "+44##########",
    "+45########",
    "+46##########",
    "+47########",
    "+48##########",
    "+49##########",
    "+51#########",
    "+52##########",
    "+53##########",
    "+54##########",
    "+55#########",
    "+56##########",
    "+57##########",
    "+58##########",
    "+60##########",
    "+61##########",
    "+62##########",
    "+63##########",
    "+64##########",
    "+65##########",
    "+66##########",
    "+81##########",
    "+82##########",
    "+84##########",
    "+86##########",
    "+90##########",
    "+91##########",
    "+92##########",
    "+93#########",
    "+94#########",
    "+95##########",
    "+98##########",
    "+211#########",
    "+212#########",
    "+213#########",
    "+216########",
    "+218#########",
    "+220#######",
    "+221#########",
    "+222########",
    "+223########",
    "+224#########",
    "+225##########",
    "+226########",
    "+227########",
    "+228########",
    "+229########",
    "+230########",
    "+231#########",
    "+232########",
    "+233#########",
    "+234##########",
    "+235########",
    "+236########",
    "+237#########",
    "+238#######",
    "+239#######",
    "+240#########",
    "+241########",
    "+242#########",
    "+243#########",
    "+244#########",
    "+245#########",
    "+246#######",
    "+247######",
    "+248#######",
    "+249#########",
    "+250#########",
    "+251#########",
    "+252#########",
    "+253########",
    "+254##########",
    "+255#########",
    "+256#########",
    "+257########",
    "+258#########",
    "+260#########",
    "+261#########",
    "+262#########",
    "+263##########",
    "+264#########",
    "+265#########",
    "+266########",
    "+267##########",
    "+268#########",
    "+269#######",
    "+290#####",
    "+291#######",
    "+297#######",
    "+298######",
    "+299######",
    "+350########",
    "+351#########",
    "+352##########",
    "+353##########",
    "+354#########",
    "+355#########",
    "+356########",
    "+357########",
    "+358##########",
    "+359#########",
    "+370########",
    "+371########",
    "+372##########",
    "+373########",
    "+374########",
    "+375##########",
    "+376#########",
    "+377#########",
    "+378##########",
    "+380##########",
    "+381##########",
    "+382#########",
    "+383#########",
    "+385#########",
    "+386########",
    "+387#########",
    "+389########",
    "+420#########",
    "+421#########",
    "+423#########",
    "+500#####",
    "+501#######",
    "+502########",
    "+503########",
    "+504########",
    "+505########",
    "+506########",
    "+507##########",
    "+508#########",
    "+509########",
    "+590#########",
    "+591#########",
    "+592#######",
    "+593##########",
    "+594#########",
    "+595##########",
    "+596#########",
    "+597#######",
    "+598##########",
    "+599########",
    "+670########",
    "+672######",
    "+673#######",
    "+674#######",
    "+675########",
    "+676#######",
    "+677#######",
    "+678#######",
    "+679#######",
    "+680#######",
    "+681#########",
    "+682#####",
    "+683#######",
    "+685##########",
    "+686########",
    "+687######",
    "+688#######",
    "+689#########",
    "+690#######",
    "+691#######",
    "+692#######",
    "+800########",
    "+808########",
    "+850##########",
    "+852#########",
    "+853########",
    "+855##########",
    "+856##########",
    "+870#########",
    "+878############",
    "+880##########",
    "+881#########",
    "+882##########",
    "+883#########",
    "+886##########",
    "+888###########",
    "+960##########",
    "+961########",
    "+962#########",
    "+963#########",
    "+964##########",
    "+965########",
    "+966##########",
    "+967#########",
    "+968#########",
    "+970##########",
    "+971##########",
    "+972##########",
    "+973########",
    "+974########",
    "+975########",
    "+976##########",
    "+977##########",
    "+979#########",
    "+992#########",
    "+993########",
    "+994#########",
    "+995#########",
    "+996#########",
    "+998#########",
];

pub(super) const INTERNATIONAL_DE_AT: &[&str] = &["+43##########"];

pub(super) const INTERNATIONAL_DE_CH: &[&str] = &["+41##########"];

pub(super) const INTERNATIONAL_EN_GB: &[&str] = &["+44##########"];

pub(super) const INTERNATIONAL_FR_FR: &[&str] = &["+33#########"];

pub(super) const INTERNATIONAL_TR_TR: &[&str] = &[
    "+90212#######",
    "+90216#######",
    "+90222#######",
    "+90224#######",
    "+90226#######",
    "+90228#######",
    "+90232#######",
    "+90236#######",
    "+90242#######",
    "+90246#######",
    "+90248#######",
    "+90252#######",
    "+90256#######",
    "+90258#######",
    "+90262#######",
    "+90264#######",
    "+90266#######",
    "+90272#######",
    "+90274#######",
    "+90276#######",
    "+90282#######",
    "+90284#######",
    "+90286#######",
    "+90288#######",
    "+90312#######",
    "+90318#######",
    "+90322#######",
    "+90324#######",
    "+90326#######",
    "+90328#######",
    "+90332#######",
    "+90338#######",
    "+90342#######",
    "+90344#######",
    "+90346#######",
    "+90348#######",
    "+90352#######",
    "+90354#######",
    "+90356#######",
    "+90358#######",
    "+90362#######",
    "+90364#######",
    "+90366#######",
    "+90368#######",
    "+90370#######",
    "+90372#######",
    "+90374#######",
    "+90376#######",
    "+90378#######",
    "+90380#######",
    "+90382#######",
    "+90384#######",
    "+90386#######",
    "+90388#######",
    "+90412#######",
    "+90414#######",
    "+90416#######",
    "+90422#######",
    "+90424#######",
    "+90426#######",
    "+90428#######",
    "+90432#######",
    "+90434#######",
    "+90436#######",
    "+90438#######",
    "+90442#######",
    "+90446#######",
    "+90452#######",
    "+90454#######",
    "+90456#######",
    "+90458#######",
    "+90462#######",
    "+90464#######",
    "+90466#######",
    "+90472#######",
    "+90474#######",
    "+90476#######",
    "+90478#######",
    "+90482#######",
    "+90484#######",
    "+90486#######",
    "+90488#######",
    "+90501#######",
    "+90502#######",
    "+90503#######",
    "+90504#######",
    "+90505#######",
    "+90506#######",
    "+90507#######",
    "+90530#######",
    "+90531#######",
    "+90532#######",
    "+90533#######",
    "+90534#######",
    "+90535#######",
    "+90536#######",
    "+90537#######",
    "+90538#######",
    "+90539#######",
    "+90541#######",
    "+90542#######",
    "+90543#######",
    "+90544#######",
    "+90545#######",
    "+90546#######",
    "+90549#######",
    "+90551#######",
    "+90552#######",
    "+90553#######",
    "+90554#######",
    "+90555#######",
    "+90556#######",
];

pub(super) const INTERNATIONAL_UK_UA: &[&str] = &[
    "+38050#######",
    "+38066#######",
    "+38068#######",
    "+38096#######",
    "+38067#######",
    "+38091#######",
    "+38092#######",
    "+38093#######",
    "+38094#######",
    "+38095#######",
    "+38096#######",
    "+38097#######",
    "+38098#######",
    "+38063#######",
    "+38099#######",
];

pub(super) const AREA_EN_US: &[&str] = &[
    "(0[1-35-9]|1[02-9]|2[03-589]|3[149]|4[08]|5[1-46]|6[0279]|7[0269]|8[13])",
    "(0[1-57-9]|1[02-9]|2[0135]|3[0-24679]|4[167]|5[12]|6[014]|8[056])",
    "(0[124-9]|1[02-579]|2[3-5]|3[0245]|4[0235]|58|6[39]|7[0589]|8[04])",
    "(0[1-57-9]|1[0235-8]|20|3[0149]|4[01]|5[19]|6[1-47]|7[013-5]|8[056])",
    "(0[1-35-9]|1[024-9]|2[03689]|[34][016]|5[017]|6[0-279]|78|8[0-29])",
    "(0[1-46-8]|1[2-9]|2[04-7]|3[1247]|4[037]|5[47]|6[02359]|7[02-59]|8[156])",
    "(0[1-68]|1[02-8]|2[08]|3[0-28]|4[3578]|5[046-9]|6[02-5]|7[028])",
    "(0[1346-9]|1[02-9]|2[0589]|3[0146-8]|4[0179]|5[12469]|7[0-389]|8[04-69])",
];

pub(super) const AREA_DE_DE: &[&str] = &[
    "(0[0-389]|0[4-6][1-68]|1[124]|1[0-9][0-9]|2[18]|2[0-9][1-9]|3[14]|3[0-35-9][0-9]|4[1]|4[02-8][0-9]|5[1]|5[02-9][0-9]|6[1]|6[02-9][0-9]|7[1]|7[2-7][0-9]|8[1]|8[02-7][0-9]|9[1]|9[02-9][0-9])",
    "(0|3[15]|3[02-46-9][1-9]|3[02-46-9][02-9][0-9]|4[015]|4[2-4679][1-8]|4[2-4679][02-9][0-9]|5[15]|5[02-46-9][1-9]|5[02-46-9][02-9][0-9]|6[15]|6[02-46-9][1-9]|6[02-46-9][02-9][0-9]|7[15]|7[2-467][1-7]|7[2-467][02-689][0-9]|8[15]|8[2-46-8][013-9]|8[2-46-8][02-9][0-9]|9[15]|9[02-46-9][1-9]|9[02-46-9][02-9][0-9])",
    "(0|1[02-9][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[0-9][0-9]|5[1]|5[02-6][0-9]|6[1]|6[02-8][0-9]|7[1]|7[02-79][0-9]|8[1]|8[02-9][0-9]|9[1]|9[02-7][0-9])",
    "(0[2-8][0-9]|1[1]|1[02-9][0-9]|2[1]|2[02-9][1-9]|3[1]|3[02-8][0-9]|4[1]|4[02-9][1-9]|5[1]|5[02-9][0-9]|6[1]|6[02-9][0-9]|7[1]|7[02-7][1-9]|8[1]|8[02-8][0-9]|9[1]|9[0-7][1-9])",
    "(0[02-9][0-9]|1[1]|1[02-9][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[0-8][0-9]|5[1]|5[02-9][0-9]|6[1]|6[2-9][0-9]|7[1]|7[02-8][1-9]|8[1]|8[02-9][1-9]|9)",
    "(0[2-8][1-6]|1[1]|1[2-9][0-9]|2[1]|2[0-7][0-9]|3[1]|3[02-9][0-9]|4[1]|4[0-8][0-9]|5[1]|5[02-8][0-9]|6[1]|6[02-8][0-9]|7[1]|7[02-7][0-9]|8[1]|8[02-5][1-9]|9[1]|9[03-7][0-9])",
    "(0[2-9][0-9]|1[1]|1[02-79][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[02-6][0-9]|5[1]|5[02-9][0-9]|6[1]|6[2-8][0-9]|7[1]|7[02-8][1-9]|8[1]|8[02-6][0-9]|9)",
    "(0[6]|0[07-9][0-9]|1[1]|1[02-9][0-9]|2[1]|2[02-9][0-9]|3[1]|3[02-9][0-9]|4[1]|4[02-9][0-9]|5[1]|5[02-7][0-9]|6[1]|6[02-8][1-9]|7[1]|7[02-467][0-9]|8[1]|8[02-7][0-9]|9[1]|9[02-7][0-9])",
];

pub(super) const TAIWAN: &[&str] = &[
    "+8869########",
    "+886-9##-###-###",
    "09########",
    "09##-###-###",
    "(02)########",
    "(02)####-####",
    "(0#)#######",
    "(0#)###-####",
    "(0##)######",
    "(0##)###-###",
];

pub(super) const CHINA_OPERATORS: &[&str] = &[
    "134", "135", "136", "137", "138", "139", "147", "150", "151", "152", "157", "158", "159",
    "1705", "178", "182", "183", "184", "187", "188", "130", "131", "132", "145", "155", "156",
    "1707", "1708", "1709", "1718", "1719", "176", "185", "186", "133", "153", "1700", "1701",
    "177", "180", "181", "189", "170", "171",
];

use super::{Faker, UniqueFaker};
use crate::database::{DatabaseError, DatabaseResult};

const DE_CH_MOBILE: &[&str] = &[
    "075 ### ## ##",
    "075#######",
    "076 ### ## ##",
    "076#######",
    "077 ### ## ##",
    "077#######",
    "078 ### ## ##",
    "078#######",
    "079 ### ## ##",
    "079#######",
];
const DE_DE_MOBILE: &[&str] = &[
    "+49{{mobileCode}}#######",
    "+49 {{mobileCode}} ### ####",
    "0{{mobileCode}}#######",
    "0{{mobileCode}} ### ####",
    "0 {{mobileCode}} ### ####",
];
const EL_CY_MOBILE: &[&str] = &["9#######"];
const EL_GR_MOBILE: &[&str] = &[
    "{{internationalCodePrefix}}{{mobileCode}}#######",
    "{{internationalCodePrefix}} {{mobileCode}} ### ####",
];
const EN_AU_MOBILE: &[&str] = &[
    "04## ### ###",
    "04##-###-###",
    "04##.###.###",
    "+61 4## ### ###",
    "+61-4##-###-###",
    "+61.4##.###.###",
];
const EN_GB_MOBILE: &[&str] = &["07#########", "07### ######", "07### ### ###"];
const EN_HK_MOBILE: &[&str] = &["5#######", "6#######", "9#######"];
const EN_IN_MOBILE: &[&str] = &[
    "+91 9#########",
    "+91 8#########",
    "+91 7#########",
    "09#########",
    "08#########",
    "07#########",
];
const EN_NZ_MOBILE: &[&str] = &["02########", "02#########", "02# ### ####", "02# #### ####"];
const EN_PH_MOBILE: &[&str] = &[
    "+63 (813) ###-####",
    "+63 (817) ###-####",
    "+63 (900) ###-####",
    "+63 (905) ###-####",
    "+63 (906) ###-####",
    "+63 (907) ###-####",
    "+63 (908) ###-####",
    "+63 (909) ###-####",
    "+63 (910) ###-####",
    "+63 (911) ###-####",
    "+63 (912) ###-####",
    "+63 (915) ###-####",
    "+63 (916) ###-####",
    "+63 (917) ###-####",
    "+63 (918) ###-####",
    "+63 (919) ###-####",
    "+63 (920) ###-####",
    "+63 (921) ###-####",
    "+63 (922) ###-####",
    "+63 (923) ###-####",
    "+63 (925) ###-####",
    "+63 (926) ###-####",
    "+63 (927) ###-####",
    "+63 (928) ###-####",
    "+63 (929) ###-####",
    "+63 (930) ###-####",
    "+63 (931) ###-####",
    "+63 (932) ###-####",
    "+63 (933) ###-####",
    "+63 (934) ###-####",
    "+63 (935) ###-####",
    "+63 (936) ###-####",
    "+63 (937) ###-####",
    "+63 (938) ###-####",
    "+63 (939) ###-####",
    "+63 (940) ###-####",
    "+63 (942) ###-####",
    "+63 (943) ###-####",
    "+63 (944) ###-####",
    "+63 (946) ###-####",
    "+63 (947) ###-####",
    "+63 (948) ###-####",
    "+63 (949) ###-####",
    "+63 (971) ###-####",
    "+63 (973) ###-####",
    "+63 (974) ###-####",
    "+63 (975) ###-####",
    "+63 (977) ###-####",
    "+63 (978) ###-####",
    "+63 (979) ###-####",
    "+63 (980) ###-####",
    "+63 (989) ###-####",
    "+63 (994) ###-####",
    "+63 (996) ###-####",
    "+63 (997) ###-####",
    "+63 (998) ###-####",
    "+63 (999) ###-####",
    "(0813) ###-####",
    "(0817) ###-####",
    "(0900) ###-####",
    "(0905) ###-####",
    "(0906) ###-####",
    "(0907) ###-####",
    "(0908) ###-####",
    "(0909) ###-####",
    "(0910) ###-####",
    "(0911) ###-####",
    "(0912) ###-####",
    "(0915) ###-####",
    "(0916) ###-####",
    "(0917) ###-####",
    "(0918) ###-####",
    "(0919) ###-####",
    "(0920) ###-####",
    "(0921) ###-####",
    "(0922) ###-####",
    "(0923) ###-####",
    "(0925) ###-####",
    "(0926) ###-####",
    "(0927) ###-####",
    "(0928) ###-####",
    "(0929) ###-####",
    "(0930) ###-####",
    "(0931) ###-####",
    "(0932) ###-####",
    "(0933) ###-####",
    "(0934) ###-####",
    "(0935) ###-####",
    "(0936) ###-####",
    "(0937) ###-####",
    "(0938) ###-####",
    "(0939) ###-####",
    "(0940) ###-####",
    "(0942) ###-####",
    "(0943) ###-####",
    "(0944) ###-####",
    "(0946) ###-####",
    "(0947) ###-####",
    "(0948) ###-####",
    "(0949) ###-####",
    "(0971) ###-####",
    "(0973) ###-####",
    "(0974) ###-####",
    "(0975) ###-####",
    "(0977) ###-####",
    "(0978) ###-####",
    "(0979) ###-####",
    "(0980) ###-####",
    "(0989) ###-####",
    "(0994) ###-####",
    "(0996) ###-####",
    "(0997) ###-####",
    "(0998) ###-####",
    "(0999) ###-####",
];
const EN_SG_MOBILE: &[&str] = &[
    "{{internationalCodePrefix}}9{{zeroToEight}}## ####",
    "{{internationalCodePrefix}} 9{{zeroToEight}}## ####",
    "9{{zeroToEight}}## ####",
    "{{internationalCodePrefix}}8{{oneToEight}}## ####",
    "{{internationalCodePrefix}} 8{{oneToEight}}## ####",
    "8{{oneToEight}}## ####",
];
const EN_ZA_MOBILE: &[&str] = &[
    "+27{{cellphoneCode}}#######",
    "0{{cellphoneCode}}#######",
    "0{{cellphoneCode}} ### ####",
    "0{{cellphoneCode}}-###-####",
];
const ES_ES_MOBILE: &[&str] = &[
    "+34 6## ## ####",
    "+34 6## ######",
    "+34 6########",
    "+34 6##-##-####",
    "+34 6##-######",
    "6## ## ####",
    "6## ######",
    "6########",
    "6##-##-####",
    "6##-######",
];
const FA_IR_MOBILE: &[&str] = &[
    "0910#######",
    "0911#######",
    "0912#######",
    "0913#######",
    "0914#######",
    "0915#######",
    "0916#######",
    "0917#######",
    "0918#######",
    "0919#######",
    "0901#######",
    "0901#######",
    "0902#######",
    "0903#######",
    "0930#######",
    "0933#######",
    "0935#######",
    "0936#######",
    "0937#######",
    "0938#######",
    "0939#######",
    "0920#######",
    "0921#######",
    "0937#######",
    "0990#######",
];
const FR_CH_MOBILE: &[&str] = &[
    "075 ### ## ##",
    "075#######",
    "076 ### ## ##",
    "076#######",
    "077 ### ## ##",
    "077#######",
    "078 ### ## ##",
    "078#######",
    "079 ### ## ##",
    "079#######",
];
const FR_FR_MOBILE: &[&str] = &[
    "+33 (0)6 {{phoneNumber06WithSeparator}}",
    "+33 6 {{phoneNumber06WithSeparator}}",
    "+33 (0)7 {{phoneNumber07WithSeparator}}",
    "+33 7 {{phoneNumber07WithSeparator}}",
    "06{{phoneNumber06}}",
    "07{{phoneNumber07}}",
    "06 {{phoneNumber06WithSeparator}}",
    "07 {{phoneNumber07WithSeparator}}",
];
const IT_CH_MOBILE: &[&str] = &[
    "075 ### ## ##",
    "075#######",
    "076 ### ## ##",
    "076#######",
    "077 ### ## ##",
    "077#######",
    "078 ### ## ##",
    "078#######",
    "079 ### ## ##",
    "079#######",
];
const MS_MY_MOBILE: &[&str] = &[
    "010-### ####",
    "011-{{zeroOneOnePrefix}}## ####",
    "012-### ####",
    "013-### ####",
    "014-{{zeroOneFourPrefix}}## ####",
    "016-### ####",
    "017-### ####",
    "018-### ####",
    "019-### ####",
];
const NB_NO_MOBILE: &[&str] = &[
    "+474#######",
    "+479#######",
    "9## ## ###",
    "4## ## ###",
    "9#######",
    "4#######",
];
const PT_PT_MOBILE: &[&str] = &["91#######", "92#######", "93#######", "96#######"];
const SV_SE_MOBILE: &[&str] = &[
    "+467########",
    "+46(0)7########",
    "+46 (0)7## ## ## ##",
    "+46 (0)7## ### ###",
    "07## ## ## ##",
    "07## ### ###",
    "07##-## ## ##",
    "07##-### ###",
    "07# ### ## ##",
    "07#-### ## ##",
    "07#-#######",
];
const TH_TH_MOBILE: &[&str] = &[
    "08# ### ####",
    "08 #### ####",
    "09# ### ####",
    "09 #### ####",
    "06# ### ####",
    "06 #### ####",
];
const FIXED_GR: &[&str] = &[
    "{{internationalCodePrefix}}21########",
    "{{internationalCodePrefix}} 21# ### ####",
    "{{internationalCodePrefix}}{{areaCode}}######",
    "{{internationalCodePrefix}} {{areaCode}} ######",
];
const FIXED_SG: &[&str] = &[
    "{{internationalCodePrefix}}6### ####",
    "{{internationalCodePrefix}} 6### ####",
    "6### ####",
];
const FIXED_MY: &[&str] = &[
    "03-#### ####",
    "04-### ####",
    "05-### ####",
    "06-### ####",
    "07-### ####",
    "08#-## ####",
    "09-### ####",
];
const FIXED_MY_COMPACT: &[&str] = &[
    "03########",
    "04#######",
    "05#######",
    "06#######",
    "07#######",
    "08#######",
    "09#######",
];
const MOBILE_MY_COMPACT: &[&str] = &[
    "010#######",
    "011{{zeroOneOnePrefix}}######",
    "012#######",
    "013#######",
    "014{{zeroOneFourPrefix}}######",
    "016#######",
    "017#######",
    "018#######",
    "019#######",
];
const LANDLINE_HK: &[&str] = &["2#######", "3#######"];
const EXTENSION_US: &[&str] = &[
    "{{areaCode}}-{{exchangeCode}}-#### x###",
    "({{areaCode}}) {{exchangeCode}}-#### x###",
    "1-{{areaCode}}-{{exchangeCode}}-#### x###",
    "{{areaCode}}.{{exchangeCode}}.#### x###",
    "{{areaCode}}-{{exchangeCode}}-#### x####",
    "({{areaCode}}) {{exchangeCode}}-#### x####",
    "1-{{areaCode}}-{{exchangeCode}}-#### x####",
    "{{areaCode}}.{{exchangeCode}}.#### x####",
    "{{areaCode}}-{{exchangeCode}}-#### x#####",
    "({{areaCode}}) {{exchangeCode}}-#### x#####",
    "1-{{areaCode}}-{{exchangeCode}}-#### x#####",
    "{{areaCode}}.{{exchangeCode}}.#### x#####",
];
const MOBILE_CODES_DE: &[&str] = &[
    "1511", "1512", "1514", "1515", "1516", "1517", "1520", "1521", "1522", "1523", "1525", "1526",
    "1529", "1570", "1573", "1575", "1577", "1578", "1579", "1590",
];
const MOBILE_CODES_GR: &[&str] = &[
    "685", "687", "688", "689", "690", "691", "693", "694", "695", "696", "697", "698", "699",
];
const AREA_CODES_GR: &[&str] = &[
    "2221", "2222", "2223", "2224", "2226", "2227", "2228", "2229", "2231", "2232", "2233", "2234",
    "2235", "2236", "2237", "2238", "2241", "2242", "2243", "2244", "2245", "2246", "2247", "2251",
    "2252", "2253", "2254", "2261", "2262", "2263", "2264", "2265", "2266", "2267", "2268", "2271",
    "2272", "2273", "2274", "2275", "2281", "2282", "2283", "2284", "2285", "2286", "2287", "2288",
    "2289", "2291", "2292", "2293", "2294", "2295", "2296", "2297", "2298", "2299", "231", "2321",
    "2322", "2323", "2324", "2325", "2327", "2331", "2332", "2333", "2341", "2343", "2351", "2352",
    "2353", "2371", "2372", "2373", "2374", "2375", "2376", "2377", "2381", "2382", "2384", "2385",
    "2386", "2391", "2392", "2393", "2394", "2395", "2396", "2397", "2399", "241", "2421", "2422",
    "2423", "2424", "2425", "2426", "2427", "2428", "2431", "2432", "2433", "2434", "2441", "2443",
    "2444", "2445", "2461", "2462", "2463", "2464", "2465", "2467", "2468", "2491", "2492", "2493",
    "2494", "2495", "251", "2521", "2522", "2523", "2524", "2531", "2532", "2533", "2534", "2535",
    "2541", "2542", "2544", "2591", "2592", "2593", "2594", "2551", "2552", "2553", "2554", "2555",
    "2556", "261", "2621", "2622", "2623", "2624", "2625", "2626", "2631", "2632", "2634", "2635",
    "2661", "2662", "2663", "2664", "2665", "2666", "2691", "2692", "2693", "2694", "2695", "2696",
    "2641", "2642", "2643", "2644", "2645", "2646", "2647", "2651", "2653", "2654", "2655", "2656",
    "2657", "2658", "2659", "2671", "2674", "2681", "2682", "2683", "2684", "2685", "271", "2721",
    "2722", "2723", "2724", "2725", "2731", "2732", "2733", "2734", "2735", "2736", "2741", "2742",
    "2743", "2744", "2745", "2746", "2747", "2751", "2752", "2753", "2754", "2755", "2757", "2761",
    "2763", "2765", "2791", "2792", "2795", "2797", "281", "2821", "2822", "2823", "2824", "2825",
    "2831", "2832", "2833", "2834", "2841", "2842", "2843", "2844", "2891", "2892", "2893", "2894",
    "2895", "2897",
];
const MY_011: &[&str] = &[
    "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "22", "23", "32",
];
const MY_014: &[&str] = &["2", "3", "4", "5", "6", "7", "8", "9"];
const BR_CELL: &[&str] = &["9####-####"];
const BR_LAND: &[&str] = &["2###-####", "3###-####", "4###-####"];
const AREA_CODES_BR: &[&str] = &[
    "11", "12", "13", "14", "15", "16", "17", "18", "19", "21", "22", "24", "27", "28", "31", "32",
    "33", "34", "35", "37", "38", "41", "42", "43", "44", "45", "46", "47", "48", "49", "51", "53",
    "54", "55", "61", "62", "63", "64", "65", "66", "67", "68", "69", "71", "73", "74", "75", "77",
    "79", "81", "82", "83", "84", "85", "86", "87", "88", "89", "91", "92", "93", "94", "95", "96",
    "97", "98", "99",
];

const TOLL_US: &[&str] = &[
    "{{tollFreeAreaCode}}-{{exchangeCode}}-####",
    "({{tollFreeAreaCode}}) {{exchangeCode}}-####",
    "1-{{tollFreeAreaCode}}-{{exchangeCode}}-####",
    "{{tollFreeAreaCode}}.{{exchangeCode}}.####",
];
const TOLL_CODES_US: &[&str] = &["800", "844", "855", "866", "877", "888"];
const TOLL_DE: &[&str] = &[
    "0{{tollFreeAreaCode}} ### ####",
    "(0{{tollFreeAreaCode}}) ### ####",
    "+49{{tollFreeAreaCode}} ### ####",
];
const FR_SERVICE: &[&str] = &[
    "+33 (0)8 {{phoneNumber08WithSeparator}}",
    "+33 8 {{phoneNumber08WithSeparator}}",
    "08 {{phoneNumber08WithSeparator}}",
    "08{{phoneNumber08}}",
];
const TOLL_ES: &[&str] = &["900 ### ###", "800 ### ###"];
const TOLL_NZ: &[&str] = &[
    "0508######",
    "0508 ######",
    "0508 ### ###",
    "0800######",
    "0800 ######",
    "0800 ### ###",
];
const TOLL_ZA: &[&str] = &[
    "{{specialCode}}#######",
    "{{specialCode}} ### ####",
    "{{specialCode}}-###-####",
    "({{specialCode}})###-####",
];
const TOLL_CODES_ZA: &[&str] = &["0800", "0860", "0861", "0862"];
const GR_PERSONAL: &[&str] = &[
    "{{internationalCodePrefix}}70########",
    "{{internationalCodePrefix}} 70 #### ####",
];
const GR_TOLL: &[&str] = &[
    "{{internationalCodePrefix}}800#######",
    "{{internationalCodePrefix}} 800 ### ####",
];
const GR_SHARED: &[&str] = &[
    "{{internationalCodePrefix}}{{sharedCostCode}}#######",
    "{{internationalCodePrefix}} {{sharedCostCode}} ### ####",
];
const GR_PREMIUM: &[&str] = &[
    "{{internationalCodePrefix}}{{premiumRateCode}}#######",
    "{{internationalCodePrefix}} {{premiumRateCode}} ### ####",
];
const GR_SHARED_CODES: &[&str] = &["801", "806", "812", "825", "850", "875"];
const GR_PREMIUM_CODES: &[&str] = &["901", "909"];
const SG_VOIP: &[&str] = &[
    "{{internationalCodePrefix}}3### ####",
    "{{internationalCodePrefix}} 3### ####",
    "3### ####",
];
const SG_TOLL_INTL: &[&str] = &["800 ### ####"];
const SG_TOLL_LINE: &[&str] = &["1800 ### ####"];
const SG_PREMIUM: &[&str] = &["1900 ### ####"];
const MY_VOIP: &[&str] = &["015-{{zeroOneFivePrefix}}## ####"];
const MY_VOIP_COMPACT: &[&str] = &["015{{zeroOneFivePrefix}}######"];
const MY_015: &[&str] = &["1", "2", "3", "4", "5", "6", "9"];
const RO_TOLL: &[&str] = &[
    "0800######",
    "0801######",
    "0802######",
    "0806######",
    "0807######",
    "0870######",
];
const RO_PREMIUM: &[&str] = &["0900######", "0903######", "0906######"];

fn mobile_formats(locale: &str) -> Option<&'static [&'static str]> {
    Some(match locale {
        "de_CH" => DE_CH_MOBILE,
        "de_DE" => DE_DE_MOBILE,
        "el_CY" => EL_CY_MOBILE,
        "el_GR" => EL_GR_MOBILE,
        "en_AU" => EN_AU_MOBILE,
        "en_GB" => EN_GB_MOBILE,
        "en_HK" => EN_HK_MOBILE,
        "en_IN" => EN_IN_MOBILE,
        "en_NZ" => EN_NZ_MOBILE,
        "en_PH" => EN_PH_MOBILE,
        "en_SG" => EN_SG_MOBILE,
        "en_ZA" => EN_ZA_MOBILE,
        "es_ES" => ES_ES_MOBILE,
        "fa_IR" => FA_IR_MOBILE,
        "fr_CH" => FR_CH_MOBILE,
        "fr_FR" => FR_FR_MOBILE,
        "it_CH" => IT_CH_MOBILE,
        "ms_MY" => MS_MY_MOBILE,
        "nb_NO" => NB_NO_MOBILE,
        "pt_PT" => PT_PT_MOBILE,
        "sv_SE" => SV_SE_MOBILE,
        "th_TH" => TH_TH_MOBILE,
        _ => return None,
    })
}

pub(super) fn uses_us_phone_provider(locale: &str) -> bool {
    match locale {
        "en_US" | "en_CA" => true,
        "bg_BG" | "bn_BD" | "cs_CZ" | "da_DK" | "de_AT" | "de_CH" | "de_DE" | "el_CY" | "el_GR"
        | "en_AU" | "en_GB" | "en_HK" | "en_IN" | "en_NG" | "en_NZ" | "en_PH" | "en_SG"
        | "en_UG" | "en_ZA" | "es_AR" | "es_ES" | "es_PE" | "es_VE" | "fa_IR" | "fi_FI"
        | "fr_BE" | "fr_CH" | "fr_FR" | "he_IL" | "hr_HR" | "hu_HU" | "hy_AM" | "id_ID"
        | "is_IS" | "it_CH" | "it_IT" | "ja_JP" | "ka_GE" | "kk_KZ" | "ko_KR" | "lt_LT"
        | "lv_LV" | "me_ME" | "mn_MN" | "ms_MY" | "nb_NO" | "ne_NP" | "nl_BE" | "nl_NL"
        | "pl_PL" | "pt_BR" | "pt_PT" | "ro_MD" | "ro_RO" | "ru_RU" | "sk_SK" | "sl_SI"
        | "sv_SE" | "th_TH" | "tr_TR" | "uk_UA" | "vi_VN" | "zh_CN" | "zh_TW" => false,
        _ => true,
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    /// Generates a display number from the selected PhoneNumber provider.
    pub fn phoneNumber(&self) -> String {
        match self.locale() {
            "zh_CN" => {
                let prefix = self.random.sample(CHINA_OPERATORS);
                format!(
                    "{prefix}{}",
                    self.numerify(Some(&"###########"[prefix.len()..]))
                )
            }
            "pt_BR" => self.brazilian_display_number(true),
            "bn_BD" => {
                let number = self.random.u32(0..=9_999_999);
                format!("+880{number}")
                    .chars()
                    .map(|digit| {
                        digit.to_digit(10).map_or(digit, |value| {
                            char::from_u32('০' as u32 + value).expect("fixed Bangla digit")
                        })
                    })
                    .collect()
            }
            "ro_RO" => self.sample_phone_template(
                if self.random.bool() {
                    DISPLAY_RO_MOBILE
                } else {
                    DISPLAY_RO_LANDLINE
                },
                false,
            ),
            "vi_VN" => {
                let area = self.random.sample(VI_AREA);
                let masks = if area.len() < 2 {
                    DISPLAY_VI_8
                } else {
                    DISPLAY_VI_7
                };
                self.numerify(Some(&self.random.sample(masks).replace("[a]", area)))
            }
            locale => self.sample_phone_template(display_formats(locale), false),
        }
    }

    /// Generates a Brazilian display number without punctuation.
    pub fn phoneNumberCleared(&self) -> DatabaseResult<String> {
        if self.locale() != "pt_BR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker cleared phone number locale",
            ));
        }
        Ok(self.brazilian_display_number(false))
    }

    fn brazilian_display_number(&self, formatting: bool) -> String {
        let formats: &[&str] = if self.random.bool() { BR_CELL } else { BR_LAND };
        self.brazilian_phone_number(formats, formatting)
    }

    pub fn mobileNumber(&self) -> DatabaseResult<String> {
        if self.locale() == "ms_MY" {
            return self.mobileNumberWithOptions(true, true);
        }
        let formats = mobile_formats(self.locale()).ok_or(DatabaseError::InvalidQuery(
            "unsupported Faker mobile number locale",
        ))?;
        Ok(self.sample_phone_template(formats, true))
    }

    pub fn mobileNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.malaysian_phone_number(
            if formatting {
                MS_MY_MOBILE
            } else {
                MOBILE_MY_COMPACT
            },
            countryCodePrefix,
            formatting,
        )
    }

    pub fn fixedLineNumber(&self) -> DatabaseResult<String> {
        match self.locale() {
            "ms_MY" => self.fixedLineNumberWithOptions(true, true),
            "el_GR" => Ok(self.sample_phone_template(FIXED_GR, true)),
            "en_SG" => Ok(self.sample_phone_template(FIXED_SG, false)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker fixed line number locale",
            )),
        }
    }

    pub fn fixedLineNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.malaysian_phone_number(
            if formatting {
                FIXED_MY
            } else {
                FIXED_MY_COMPACT
            },
            countryCodePrefix,
            formatting,
        )
    }

    pub fn mobilePhoneNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "el_GR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker mobile phone number alias locale",
            ));
        }
        // The upstream compatibility formatter preserves leading whitespace.
        Ok(self.sample_phone_template(EL_GR_MOBILE, false))
    }

    /// The Brazilian formatter and its case-insensitive PHP alias in Korean.
    pub fn cellphoneNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        match self.locale() {
            "pt_BR" => Ok(self.brazilian_phone_number(BR_CELL, formatted.unwrap_or(true))),
            "ko_KR" => Ok(self.numerify(Some("010-####-####"))),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker cellphone number locale",
            )),
        }
    }

    pub fn cellPhoneNumber(&self) -> DatabaseResult<String> {
        self.cellphoneNumber(None)
    }

    pub fn landlineNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        match self.locale() {
            "pt_BR" => Ok(self.brazilian_phone_number(BR_LAND, formatted.unwrap_or(true))),
            "en_HK" => Ok(self.sample_phone_template(LANDLINE_HK, false)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker landline number locale",
            )),
        }
    }

    pub fn phoneNumberWithExtension(&self) -> DatabaseResult<String> {
        if !uses_us_phone_provider(self.locale()) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker phone extension locale",
            ));
        }
        Ok(self.sample_phone_template(EXTENSION_US, false))
    }

    /// Generate a regional VoIP sample with the provider's default formatting.
    pub fn voipNumber(&self) -> DatabaseResult<String> {
        match self.locale() {
            "ms_MY" => self.voipNumberWithOptions(true, true),
            "en_SG" => Ok(self.sample_phone_template(SG_VOIP, false)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker VoIP number locale",
            )),
        }
    }

    /// Control the prefix and separators for the Malaysian VoIP format.
    pub fn voipNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.malaysian_phone_number(
            if formatting { MY_VOIP } else { MY_VOIP_COMPACT },
            countryCodePrefix,
            formatting,
        )
    }

    pub fn tollFreePhoneNumber(&self) -> DatabaseResult<String> {
        let formats = match self.locale() {
            "de_DE" => TOLL_DE,
            "ro_RO" => RO_TOLL,
            locale if uses_us_phone_provider(locale) => TOLL_US,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker toll-free phone number locale",
                ));
            }
        };
        Ok(self.sample_phone_template(formats, false))
    }

    /// Samples a regional area code while preserving any leading zeros.
    pub fn areaCode(&self) -> DatabaseResult<String> {
        let locale = self.locale();
        if !matches!(
            locale,
            "de_DE" | "el_GR" | "en_AU" | "en_NZ" | "en_ZA" | "pt_BR"
        ) && !uses_us_phone_provider(locale)
        {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker area code locale",
            ));
        }
        Ok(self.sample_phone_template(&["{{areaCode}}"], false))
    }

    pub fn exchangeCode(&self) -> DatabaseResult<String> {
        if !uses_us_phone_provider(self.locale()) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker exchange code locale",
            ));
        }
        Ok(self.us_phone_exchange_code())
    }

    pub fn tollFreeAreaCode(&self) -> DatabaseResult<String> {
        if self.locale() != "de_DE" && !uses_us_phone_provider(self.locale()) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker toll-free area code locale",
            ));
        }
        Ok(self.sample_phone_template(&["{{tollFreeAreaCode}}"], false))
    }

    pub fn countryCodePrefix(&self, formatting: bool) -> DatabaseResult<&'static str> {
        if self.locale() != "ms_MY" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker country code prefix locale",
            ));
        }
        Ok(if formatting { "+6" } else { "6" })
    }

    /// Samples a subscriber mask. Number templates fill the placeholders when rendered.
    pub fn numberFormat(&self) -> DatabaseResult<&'static str> {
        let formats = match self.locale() {
            "hy_AM" => AM_FORMAT,
            "fi_FI" => FI_FORMAT,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker phone number mask locale",
                ));
            }
        };
        Ok(self.random.sample(formats))
    }

    pub fn separator(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "fi_FI" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker phone separator locale",
            ));
        }
        Ok(self.random.sample(&[" ", "-"]))
    }

    pub fn cellphone(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.brazilian_subscriber(BR_CELL, formatted.unwrap_or(true))
    }

    pub fn landline(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.brazilian_subscriber(BR_LAND, formatted.unwrap_or(true))
    }

    pub fn phone(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        // Two of the three upstream choices select the cellphone group.
        self.brazilian_subscriber(
            if self.random.u8(0..3) == 0 {
                BR_LAND
            } else {
                BR_CELL
            },
            formatted.unwrap_or(true),
        )
    }

    fn brazilian_subscriber(&self, formats: &[&str], formatted: bool) -> DatabaseResult<String> {
        if self.locale() != "pt_BR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Brazilian subscriber number locale",
            ));
        }
        let number = self.sample_phone_template(formats, false);
        Ok(if formatted {
            number
        } else {
            number.replace('-', "")
        })
    }

    fn brazilian_phone_number(&self, formats: &[&str], formatted: bool) -> String {
        let area = self.random.sample(AREA_CODES_BR);
        let number = self.sample_phone_template(formats, false);
        if formatted {
            format!("({area}) {number}")
        } else {
            format!("{area}{}", number.replace('-', ""))
        }
    }

    fn malaysian_phone_number(
        &self,
        formats: &[&str],
        country_code_prefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        let prefix = self.countryCodePrefix(formatting)?;
        let prefix = if country_code_prefix { prefix } else { "" };
        Ok(format!(
            "{prefix}{}",
            self.sample_phone_template(formats, false)
        ))
    }

    fn sample_phone_template(&self, formats: &[&str], trim_start: bool) -> String {
        let mut format = self.random.sample(formats);
        let mut rendered = String::new();
        while let Some((literal, tail)) = format.split_once("{{") {
            let (token, remaining) = tail.split_once("}}").expect("fixed phone template");
            rendered.push_str(literal);
            self.append_phone_token(&mut rendered, token.trim());
            format = remaining;
        }
        rendered.push_str(format);
        self.numerify(Some(if trim_start {
            rendered.trim_start()
        } else {
            &rendered
        }))
    }

    fn append_phone_token(&self, rendered: &mut String, token: &str) {
        let literal = match (self.locale(), token) {
            ("el_GR", "internationalCodePrefix") => self.random.sample(&["", "+30"]),
            ("en_SG", "internationalCodePrefix") => self.random.sample(&["+65", "65"]),
            ("el_GR", "mobileCode") => self.random.sample(MOBILE_CODES_GR),
            ("de_DE", "mobileCode") => self.random.sample(MOBILE_CODES_DE),
            ("en_SG", "zeroToEight") => {
                return rendered.push(char::from(b'0' + self.random.u8(0..=8)));
            }
            ("en_SG", "oneToEight") => {
                return rendered.push(char::from(b'0' + self.random.u8(1..=8)));
            }
            ("ms_MY", "zeroOneOnePrefix") => self.random.sample(MY_011),
            ("ms_MY", "zeroOneFourPrefix") => self.random.sample(MY_014),
            ("ms_MY", "zeroOneFivePrefix") => self.random.sample(MY_015),
            ("el_GR", "sharedCostCode") => self.random.sample(GR_SHARED_CODES),
            ("el_GR", "premiumRateCode") => self.random.sample(GR_PREMIUM_CODES),
            ("en_ZA", "specialCode") => self.random.sample(TOLL_CODES_ZA),
            ("de_DE", "tollFreeAreaCode") => "800",
            (_, "tollFreeAreaCode") => self.random.sample(TOLL_CODES_US),
            ("fr_FR", "phoneNumber08" | "phoneNumber08WithSeparator") => {
                // FakerPHP's regex generator attaches the three trailing pairs to either branch.
                rendered.push_str(if self.random.bool() {
                    self.random.sample(&["0#", "1#", "2#"])
                } else {
                    self.random
                        .sample(&["91", "92", "93", "95", "97", "98", "99"])
                });
                if token.ends_with("WithSeparator") {
                    " ## ## ##"
                } else {
                    "######"
                }
            }
            ("el_GR", "areaCode") => {
                let code = self.random.sample(AREA_CODES_GR);
                rendered.push_str(code);
                return rendered.push_str(&"####"[code.len()..]);
            }
            ("en_ZA", "cellphoneCode") => {
                let first = self.random.u8(6..=8);
                let second = match first {
                    6 => self.random.u8(0..=2),
                    7 => match self.random.u8(1..=9) {
                        value @ (5 | 7) => value + 1,
                        value => value,
                    },
                    _ => self.random.u8(1..=9),
                };
                rendered.push(char::from(b'0' + first));
                return rendered.push(char::from(b'0' + second));
            }
            ("fr_FR", "phoneNumber06" | "phoneNumber06WithSeparator") => {
                match self.random.u8(0..3) {
                    0 => {
                        rendered.push_str(
                            self.random
                                .sample(&["0", "1", "2", "4", "5", "6", "7", "8"]),
                        );
                        rendered.push('#');
                    }
                    1 => {
                        rendered.push('3');
                        rendered.push(char::from(b'0' + self.random.u8(0..=8)));
                    }
                    _ => {
                        rendered.push('9');
                        rendered.push_str(self.random.sample(&["5", "8", "9"]));
                    }
                }
                if token.ends_with("WithSeparator") {
                    " ## ## ##"
                } else {
                    "######"
                }
            }
            ("fr_FR", "phoneNumber07" | "phoneNumber07WithSeparator") => {
                rendered.push(char::from(b'0' + self.random.u8(3..=8)));
                if token.ends_with("WithSeparator") {
                    "# ## ## ##"
                } else {
                    "#######"
                }
            }
            ("de_DE", "areaCode") => return rendered.push_str(&self.de_phone_area_code()),
            ("pt_BR", "areaCode") => self.random.sample(AREA_CODES_BR),
            ("en_AU", "areaCode") => self.random.sample(AU_AREA),
            ("en_NZ", "areaCode") => self.random.sample(NZ_AREA),
            ("en_NZ", "beginningNumber") => self.random.sample(NZ_BEGINNING),
            ("en_ZA", "areaCode") => {
                let first = self.random.u8(1..=5);
                let second = match (first, self.random.u8(1..=if first == 4 { 9 } else { 8 })) {
                    (2, digit @ (5 | 6)) | (5, digit @ (2 | 5)) => digit + 2,
                    (3, digit @ (7 | 8)) => digit - 2,
                    (_, digit) => digit,
                };
                rendered.push(char::from(b'0' + first));
                return rendered.push(char::from(b'0' + second));
            }
            ("hy_AM", "code") => self.random.sample(AM_CODE),
            (_, "numberFormat") => self.numberFormat().expect("fixed phone mask locale"),
            ("fi_FI", "landLineAreaCode") => self.random.sample(FI_LAND_CODE),
            ("fi_FI", "e164landLineAreaCode") => &self.random.sample(FI_LAND_CODE)[1..],
            ("fi_FI", "mobileNetworkAreaCode") => self.random.sample(FI_MOBILE_CODE),
            ("fi_FI", "e164MobileNetworkAreaCode") => &self.random.sample(FI_MOBILE_CODE)[1..],
            ("fi_FI", "separator") => self.separator().expect("fixed phone separator locale"),
            (_, "fixedLineNumber") => {
                return rendered.push_str(
                    &self
                        .fixedLineNumber()
                        .expect("fixed display template locale"),
                );
            }
            (_, "mobileNumber") => {
                return rendered
                    .push_str(&self.mobileNumber().expect("fixed display template locale"));
            }
            (_, "voipNumber") => {
                return rendered
                    .push_str(&self.voipNumber().expect("fixed display template locale"));
            }
            (_, "personalNumber") => {
                return rendered.push_str(
                    &self
                        .personalNumber()
                        .expect("fixed display template locale"),
                );
            }
            (_, "premiumRateNumber") => {
                return rendered.push_str(
                    &self
                        .premiumRateNumber()
                        .expect("fixed display template locale"),
                );
            }
            (_, "sharedCostNumber") => {
                return rendered.push_str(
                    &self
                        .sharedCostNumber()
                        .expect("fixed display template locale"),
                );
            }
            (_, "tollFreeNumber") => {
                return rendered.push_str(
                    &self
                        .tollFreeNumber()
                        .expect("fixed display template locale"),
                );
            }
            (_, "areaCode") => return rendered.push_str(&self.us_phone_area_code()),
            (_, "exchangeCode") => return rendered.push_str(&self.us_phone_exchange_code()),
            _ => unreachable!("fixed phone template token"),
        };
        rendered.push_str(literal);
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn areaCode(&self) -> DatabaseResult<String> {
        self.try_generate("areaCode", || self.0.areaCode())
    }

    pub fn exchangeCode(&self) -> DatabaseResult<String> {
        self.try_generate("exchangeCode", || self.0.exchangeCode())
    }

    pub fn tollFreeAreaCode(&self) -> DatabaseResult<String> {
        self.try_generate("tollFreeAreaCode", || self.0.tollFreeAreaCode())
    }

    pub fn numberFormat(&self) -> DatabaseResult<&'static str> {
        self.try_generate("numberFormat", || self.0.numberFormat())
    }

    pub fn separator(&self) -> DatabaseResult<&'static str> {
        self.try_generate("separator", || self.0.separator())
    }

    pub fn countryCodePrefix(&self, formatting: bool) -> DatabaseResult<&'static str> {
        self.try_generate("countryCodePrefix", || self.0.countryCodePrefix(formatting))
    }

    pub fn cellphone(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("cellphone", || self.0.cellphone(formatted))
    }

    pub fn landline(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("landline", || self.0.landline(formatted))
    }

    pub fn phone(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("phone", || self.0.phone(formatted))
    }

    pub fn phoneNumberCleared(&self) -> DatabaseResult<String> {
        self.try_generate("phoneNumberCleared", || self.0.phoneNumberCleared())
    }

    pub fn mobileNumber(&self) -> DatabaseResult<String> {
        self.try_generate("mobileNumber", || self.0.mobileNumber())
    }

    pub fn mobileNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("mobileNumber", || {
            self.0
                .mobileNumberWithOptions(countryCodePrefix, formatting)
        })
    }

    pub fn fixedLineNumber(&self) -> DatabaseResult<String> {
        self.try_generate("fixedLineNumber", || self.0.fixedLineNumber())
    }

    pub fn fixedLineNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("fixedLineNumber", || {
            self.0
                .fixedLineNumberWithOptions(countryCodePrefix, formatting)
        })
    }

    pub fn mobilePhoneNumber(&self) -> DatabaseResult<String> {
        self.try_generate("mobilePhoneNumber", || self.0.mobilePhoneNumber())
    }

    pub fn cellphoneNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        let formatter = if self.0.locale() == "ko_KR" {
            "cellPhoneNumber"
        } else {
            "cellphoneNumber"
        };
        self.try_generate(formatter, || self.0.cellphoneNumber(formatted))
    }

    pub fn cellPhoneNumber(&self) -> DatabaseResult<String> {
        self.cellphoneNumber(None)
    }

    pub fn landlineNumber(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("landlineNumber", || self.0.landlineNumber(formatted))
    }

    pub fn tollFreePhoneNumber(&self) -> DatabaseResult<String> {
        self.try_generate("tollFreePhoneNumber", || self.0.tollFreePhoneNumber())
    }

    pub fn voipNumber(&self) -> DatabaseResult<String> {
        self.try_generate("voipNumber", || self.0.voipNumber())
    }

    pub fn voipNumberWithOptions(
        &self,
        countryCodePrefix: bool,
        formatting: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("voipNumber", || {
            self.0.voipNumberWithOptions(countryCodePrefix, formatting)
        })
    }

    pub fn phoneNumberWithExtension(&self) -> DatabaseResult<String> {
        self.try_generate("phoneNumberWithExtension", || {
            self.0.phoneNumberWithExtension()
        })
    }
}

// These formatters differ only in their provider's masks and whitespace handling.
macro_rules! phone_formatters {
    ($($name:ident => { $($locale:pat => ($formats:expr, $trim:literal)),+ }),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(
                pub fn $name(&self) -> DatabaseResult<String> {
                    let (formats, trim): (&[&str], bool) = match self.locale() {
                        $($locale => ($formats, $trim),)+
                        _ => return Err(DatabaseError::InvalidQuery(
                            concat!("unsupported Faker ", stringify!($name), " locale"),
                        )),
                    };
                    Ok(self.sample_phone_template(formats, trim))
                }
            )+
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(
                pub fn $name(&self) -> DatabaseResult<String> {
                    self.try_generate(stringify!($name), || self.0.$name())
                }
            )+
        }
    };
}

phone_formatters! {
    mobileCode => { "de_DE" | "el_GR" => (&["{{mobileCode}}"], false) },
    internationalCodePrefix => { "el_GR" | "en_SG" => (&["{{internationalCodePrefix}}"], false) },
    sharedCostCode => { "el_GR" => (&["{{sharedCostCode}}"], false) },
    premiumRateCode => { "el_GR" => (&["{{premiumRateCode}}"], false) },
    cellphoneCode => { "en_ZA" => (&["{{cellphoneCode}}"], false) },
    specialCode => { "en_ZA" => (&["{{specialCode}}"], false) },
    beginningNumber => { "en_NZ" => (&["{{beginningNumber}}"], false) },
    code => { "hy_AM" => (&["{{code}}"], false) },
    landLineAreaCode => { "fi_FI" => (&["{{landLineAreaCode}}"], false) },
    e164landLineAreaCode => { "fi_FI" => (&["{{e164landLineAreaCode}}"], false) },
    mobileNetworkAreaCode => { "fi_FI" => (&["{{mobileNetworkAreaCode}}"], false) },
    e164MobileNetworkAreaCode => { "fi_FI" => (&["{{e164MobileNetworkAreaCode}}"], false) },
    zeroOneOnePrefix => { "ms_MY" => (&["{{zeroOneOnePrefix}}"], false) },
    zeroOneFourPrefix => { "ms_MY" => (&["{{zeroOneFourPrefix}}"], false) },
    zeroOneFivePrefix => { "ms_MY" => (&["{{zeroOneFivePrefix}}"], false) },
    oneToEight => { "en_SG" => (&["{{oneToEight}}"], false) },
    zeroToEight => { "en_SG" => (&["{{zeroToEight}}"], false) },
    phoneNumber06 => { "fr_FR" => (&["{{phoneNumber06}}"], false) },
    phoneNumber06WithSeparator => { "fr_FR" => (&["{{phoneNumber06WithSeparator}}"], false) },
    phoneNumber07 => { "fr_FR" => (&["{{phoneNumber07}}"], false) },
    phoneNumber07WithSeparator => { "fr_FR" => (&["{{phoneNumber07WithSeparator}}"], false) },
    phoneNumber08 => { "fr_FR" => (&["{{phoneNumber08}}"], false) },
    phoneNumber08WithSeparator => { "fr_FR" => (&["{{phoneNumber08WithSeparator}}"], false) },
    faxNumber => { "en_HK" => (&["7#######"], false) },
    localAreaPhoneNumber => { "ko_KR" => (&DISPLAY_KO_KR[..6], false) },
    tollFreeNumber => {
        "el_GR" => (GR_TOLL, true),
        "en_NZ" => (TOLL_NZ, false),
        "en_ZA" => (TOLL_ZA, false),
        "es_ES" => (TOLL_ES, false)
    },
    personalNumber => { "el_GR" => (GR_PERSONAL, true) },
    sharedCostNumber => { "el_GR" => (GR_SHARED, true) },
    premiumRateNumber => { "el_GR" => (GR_PREMIUM, true) },
    premiumRatePhoneNumber => { "ro_RO" => (RO_PREMIUM, false) },
    premiumPhoneNumber => { "en_SG" => (SG_PREMIUM, false) },
    tollFreeInternationalNumber => { "en_SG" => (SG_TOLL_INTL, false) },
    tollFreeLineNumber => { "en_SG" => (SG_TOLL_LINE, false) },
    serviceNumber => { "fr_FR" => (FR_SERVICE, false) },
}

const DISPLAY_BG_BG: &[&str] = &[
    "+359(0)#########",
    "+359(0)### ######",
    "+359(0)### ### ###",
    "+359#########",
    "0#########",
    "0### ######",
    "0### ### ###",
    "0### ###-###",
    "(0###) ######",
    "(0###) ### ###",
    "(0###) ###-###",
];
const DISPLAY_CS_CZ: &[&str] = &[
    "+420 %## ### ###",
    "%## ### ###",
    "00420%########",
    "+420%########",
    "%########",
];
const DISPLAY_DA_DK: &[&str] = &[
    "+45 ## ## ## ##",
    "+45 #### ####",
    "+45########",
    "## ## ## ##",
    "#### ####",
    "########",
];
const DISPLAY_DE_AT: &[&str] = &[
    "0650 #######",
    "0660 #######",
    "0664 #######",
    "0676 #######",
    "0677 #######",
    "0678 #######",
    "0699 #######",
    "0680 #######",
    "+43 #### ####",
    "+43 #### ####-##",
];
const DISPLAY_DE_CH: &[&str] = &[
    "+41 (0)## ### ## ##",
    "+41(0)#########",
    "+41 ## ### ## ##",
    "0#########",
    "0## ### ## ##",
];
const DISPLAY_DE_DE: &[&str] = &[
    "+49 {{areaCode}} #######",
    "+49 {{areaCode}} ### ####",
    "+49 (0{{areaCode}}) #######",
    "+49 (0{{areaCode}}) ### ####",
    "+49{{areaCode}}#######",
    "+49{{areaCode}}### ####",
    "0{{areaCode}} ### ####",
    "0{{areaCode}} #######",
    "(0{{areaCode}}) ### ####",
    "(0{{areaCode}}) #######",
];
const DISPLAY_EL_CY: &[&str] = &["+3572#######", "+3579#######", "2#######", "9#######"];
const DISPLAY_EL_GR: &[&str] = &[
    "{{fixedLineNumber}}",
    "{{mobileNumber}}",
    "{{personalNumber}}",
    "{{tollFreeNumber}}",
    "{{sharedCostNumber}}",
    "{{premiumRateNumber}}",
];
const DISPLAY_EN_AU: &[&str] = &[
    "#### ####",
    "####-####",
    "####.####",
    "########",
    "0{{areaCode}} #### ####",
    "0{{areaCode}}-####-####",
    "0{{areaCode}}.####.####",
    "0{{areaCode}}########",
    "(0{{areaCode}}) #### ####",
    "(0{{areaCode}})-####-####",
    "(0{{areaCode}}).####.####",
    "(0{{areaCode}})########",
    "+61 {{areaCode}} #### ####",
    "+61-{{areaCode}}-####-####",
    "+61.{{areaCode}}.####.####",
    "+61{{areaCode}}########",
];
const DISPLAY_EN_CA: &[&str] = &[
    "%##-###-####",
    "%##.###.####",
    "%## ### ####",
    "(%##) ###-####",
    "1-%##-###-####",
    "1 (%##) ###-####",
    "+1 (%##) ###-####",
    "%##-###-#### x###",
    "(%##) ###-#### x###",
];
const DISPLAY_EN_GB: &[&str] = &[
    "+44(0)##########",
    "+44(0)#### ######",
    "+44(0)#########",
    "+44(0)#### #####",
    "0##########",
    "0#########",
    "0#### ######",
    "0#### #####",
    "0### ### ####",
    "0### #######",
    "(0####) ######",
    "(0####) #####",
    "(0###) ### ####",
    "(0###) #######",
];
const DISPLAY_EN_HK: &[&str] = &["2#######", "3#######", "5#######", "6#######", "9#######"];
const DISPLAY_EN_IN: &[&str] = &[
    "+91 ## ########",
    "+91 ### #######",
    "0## ########",
    "0### #######",
];
const DISPLAY_EN_NG: &[&str] = &[
    "0703#######",
    "0704#######",
    "0705#######",
    "0706#######",
    "0707#######",
    "0708#######",
    "0709#######",
    "0802#######",
    "0803#######",
    "0804#######",
    "0805#######",
    "0806#######",
    "0807#######",
    "0808#######",
    "0809#######",
    "0810#######",
    "0811#######",
    "0812#######",
    "0813#######",
    "0814#######",
    "0815#######",
    "0816#######",
    "0817#######",
    "0818#######",
    "0819#######",
    "0902#######",
    "0903#######",
    "0905#######",
    "0908#######",
    "0909#######",
    "+234703#######",
    "+234704#######",
    "+234705#######",
    "+234706#######",
    "+234707#######",
    "+234708#######",
    "+234709#######",
    "+234802#######",
    "+234803#######",
    "+234804#######",
    "+234805#######",
    "+234806#######",
    "+234807#######",
    "+234808#######",
    "+234809#######",
    "+234810#######",
    "+234811#######",
    "+234812#######",
    "+234813#######",
    "+234814#######",
    "+234815#######",
    "+234816#######",
    "+234817#######",
    "+234818#######",
    "+234819#######",
    "+234902#######",
    "+234903#######",
    "+234905#######",
    "+234908#######",
    "+234909#######",
    "0703 ### ####",
    "0704 ### ####",
    "0705 ### ####",
    "0706 ### ####",
    "0707 ### ####",
    "0708 ### ####",
    "0709 ### ####",
    "0802 ### ####",
    "0803 ### ####",
    "0804 ### ####",
    "0805 ### ####",
    "0806 ### ####",
    "0807 ### ####",
    "0808 ### ####",
    "0809 ### ####",
    "0810 ### ####",
    "0811 ### ####",
    "0812 ### ####",
    "0813 ### ####",
    "0814 ### ####",
    "0815 ### ####",
    "0816 ### ####",
    "0817 ### ####",
    "0818 ### ####",
    "0819 ### ####",
    "0902 ### ####",
    "0903 ### ####",
    "0905 ### ####",
    "0908 ### ####",
    "0909 ### ####",
    "+234 703 ### ####",
    "+234 704 ### ####",
    "+234 705 ### ####",
    "+234 706 ### ####",
    "+234 707 ### ####",
    "+234 708 ### ####",
    "+234 709 ### ####",
    "+234 802 ### ####",
    "+234 803 ### ####",
    "+234 804 ### ####",
    "+234 805 ### ####",
    "+234 806 ### ####",
    "+234 807 ### ####",
    "+234 808 ### ####",
    "+234 809 ### ####",
    "+234 810 ### ####",
    "+234 811 ### ####",
    "+234 812 ### ####",
    "+234 813 ### ####",
    "+234 814 ### ####",
    "+234 815 ### ####",
    "+234 816 ### ####",
    "+234 817 ### ####",
    "+234 818 ### ####",
    "+234 819 ### ####",
    "+234 902 ### ####",
    "+234 903 ### ####",
    "+234 905 ### ####",
    "+234 908 ### ####",
    "+234 909 ### ####",
];
const DISPLAY_EN_NZ: &[&str] = &[
    "{{areaCode}}{{beginningNumber}}######",
    "{{areaCode}} {{beginningNumber}}## ####",
];
const DISPLAY_EN_PH: &[&str] = &["###-###-###"];
const DISPLAY_EN_SG: &[&str] = &["{{mobileNumber}}", "{{fixedLineNumber}}"];
const DISPLAY_EN_UG: &[&str] = &[
    "+256 7## ### ###",
    "+2567########",
    "+256 4## ### ###",
    "+2564########",
    "07## ### ###",
    "07########",
    "04## ### ###",
    "04########",
];
const DISPLAY_EN_US: &[&str] = &[
    "+1-{{areaCode}}-{{exchangeCode}}-####",
    "+1 ({{areaCode}}) {{exchangeCode}}-####",
    "+1-{{areaCode}}-{{exchangeCode}}-####",
    "+1.{{areaCode}}.{{exchangeCode}}.####",
    "+1{{areaCode}}{{exchangeCode}}####",
    "{{areaCode}}-{{exchangeCode}}-####",
    "({{areaCode}}) {{exchangeCode}}-####",
    "1-{{areaCode}}-{{exchangeCode}}-####",
    "{{areaCode}}.{{exchangeCode}}.####",
    "{{areaCode}}-{{exchangeCode}}-####",
    "({{areaCode}}) {{exchangeCode}}-####",
    "1-{{areaCode}}-{{exchangeCode}}-####",
    "{{areaCode}}.{{exchangeCode}}.####",
];
const DISPLAY_EN_ZA: &[&str] = &[
    "+27({{areaCode}})#######",
    "+27{{areaCode}}#######",
    "0{{areaCode}}#######",
    "0{{areaCode}} ### ####",
    "0{{areaCode}}-###-####",
];
const DISPLAY_ES_AR: &[&str] = &[
    "+##(#)##########",
    "+##(#)##########",
    "###-###-####",
    "(##)4###-####",
    "(##)4###-####",
    "(##)4###-####",
    "(##)4###-####",
    "(##)154###-####",
    "(##)5###-####",
    "(##)5###-####",
    "(##)155###-####",
    "(##)155###-####",
    "(##)155###-####",
    "(##)6###-####",
    "(###)4##-####",
    "(###)4##-####",
    "(###)4##-####",
    "(###)5##-####",
    "(###)5##-####",
    "(###)5##-####",
    "(###)154##-####",
    "(###)154##-####",
    "(###)155##-####",
    "(###)155##-####",
    "(###)155##-####",
    "(####)4#-####",
    "(####)4#-####",
    "(####)4#-####",
    "(####)4#-####",
    "(####)154#-####",
    "(####)154#-####",
    "(####)154#-####",
    "(####)155#-####",
];
const DISPLAY_ES_ES: &[&str] = &[
    "+34 9## ## ####",
    "+34 9## ######",
    "+34 9########",
    "+34 9##-##-####",
    "+34 9##-######",
    "9## ## ####",
    "9## ######",
    "9########",
    "9##-##-####",
    "9##-######",
];
const DISPLAY_ES_PE: &[&str] = &[
    "+51 9## ### ###",
    "+51 9########",
    "9## ### ###",
    "9########",
    "+51 1## ####",
    "+51 1######",
    "1## ####",
    "1######",
];
const DISPLAY_ES_VE: &[&str] = &[
    "+58 2## ### ####",
    "+58 2## #######",
    "+58 2#########",
    "+58 2##-###-####",
    "+58 2##-#######",
    "2## ### ####",
    "2## #######",
    "2#########",
    "2##-###-####",
    "2##-#######",
    "+58 4## ### ####",
    "+58 4## #######",
    "+58 4#########",
    "+58 4##-###-####",
    "+58 4##-#######",
    "4## ### ####",
    "4## #######",
    "4#########",
    "4##-###-####",
    "4##-#######",
];
const DISPLAY_FA_IR: &[&str] = &[
    "011########",
    "013########",
    "017########",
    "021########",
    "023########",
    "024########",
    "025########",
    "026########",
    "028########",
    "031########",
    "034########",
    "035########",
    "038########",
    "041########",
    "044########",
    "045########",
    "051########",
    "054########",
    "056########",
    "058########",
    "061########",
    "066########",
    "071########",
    "074########",
    "076########",
    "077########",
    "081########",
    "083########",
    "084########",
    "086########",
    "087########",
];
const DISPLAY_FI_FI: &[&str] = &[
    "+358 ({{ e164MobileNetworkAreaCode }}) {{ numberFormat }}",
    "+358 {{ e164MobileNetworkAreaCode }} {{ numberFormat }}",
    "+358 ({{ e164landLineAreaCode }}) {{ numberFormat }}",
    "+358 {{ e164landLineAreaCode }} {{ numberFormat }}",
    "{{ mobileNetworkAreaCode }}{{ separator }}{{ numberFormat }}",
    "{{ landLineAreaCode }}{{ separator }}{{ numberFormat }}",
];
const DISPLAY_FR_BE: &[&str] = &[
    "+32(0)########",
    "+32(0)### ######",
    "+32(0)# #######",
    "0#########",
    "0### ######",
    "0### ### ###",
    "0### ## ## ##",
    "0## ######",
    "0## ## ## ##",
    "0# #######",
    "0# ### ## ##",
];
const DISPLAY_FR_FR: &[&str] = &[
    "+33 (0)1 ## ## ## ##",
    "+33 (0)1 ## ## ## ##",
    "+33 (0)2 ## ## ## ##",
    "+33 (0)3 ## ## ## ##",
    "+33 (0)4 ## ## ## ##",
    "+33 (0)5 ## ## ## ##",
    "+33 (0)6 {{phoneNumber06WithSeparator}}",
    "+33 (0)7 {{phoneNumber07WithSeparator}}",
    "+33 (0)8 {{phoneNumber08WithSeparator}}",
    "+33 (0)9 ## ## ## ##",
    "+33 1 ## ## ## ##",
    "+33 1 ## ## ## ##",
    "+33 2 ## ## ## ##",
    "+33 3 ## ## ## ##",
    "+33 4 ## ## ## ##",
    "+33 5 ## ## ## ##",
    "+33 6 {{phoneNumber06WithSeparator}}",
    "+33 7 {{phoneNumber07WithSeparator}}",
    "+33 8 {{phoneNumber08WithSeparator}}",
    "+33 9 ## ## ## ##",
    "01########",
    "01########",
    "02########",
    "03########",
    "04########",
    "05########",
    "06{{phoneNumber06}}",
    "07{{phoneNumber07}}",
    "08{{phoneNumber08}}",
    "09########",
    "01 ## ## ## ##",
    "01 ## ## ## ##",
    "02 ## ## ## ##",
    "03 ## ## ## ##",
    "04 ## ## ## ##",
    "05 ## ## ## ##",
    "06 {{phoneNumber06WithSeparator}}",
    "07 {{phoneNumber07WithSeparator}}",
    "08 {{phoneNumber08WithSeparator}}",
    "09 ## ## ## ##",
];
const DISPLAY_HE_IL: &[&str] = &[
    "05#-#######",
    "0#-#######",
    "972-5#-#######",
    "972-#-########",
    "0#########",
];
const DISPLAY_HR_HR: &[&str] = &[
    "+385 91 ### ####",
    "+385 92 ### ####",
    "+385 95 ### ####",
    "+385 98 ### ####",
    "+385 99 ### ####",
];
const DISPLAY_HU_HU: &[&str] = &[
    "+36-##-###-####",
    "+36#########",
    "+36(##)###-###",
    "06-##-###-####",
    "06(##)###-###",
];
const DISPLAY_HY_AM: &[&str] = &[
    "0{{code}} {{numberFormat}}",
    "(0{{code}}) {{numberFormat}}",
    "+374{{code}} {{numberFormat}}",
    "+374 {{code}} {{numberFormat}}",
];
const DISPLAY_ID_ID: &[&str] = &[
    "02# #### ###",
    "02## #### ###",
    "03## #### ###",
    "04## #### ###",
    "05## #### ###",
    "06## #### ###",
    "07## #### ###",
    "09## #### ###",
    "02# #### ####",
    "02## #### ####",
    "03## #### ####",
    "04## #### ####",
    "05## #### ####",
    "06## #### ####",
    "07## #### ####",
    "09## #### ####",
    "08## ### ###",
    "08## #### ###",
    "08## #### ####",
    "(+62) 8## ### ###",
    "(+62) 2# #### ###",
    "(+62) 2## #### ###",
    "(+62) 3## #### ###",
    "(+62) 4## #### ###",
    "(+62) 5## #### ###",
    "(+62) 6## #### ###",
    "(+62) 7## #### ###",
    "(+62) 8## #### ###",
    "(+62) 9## #### ###",
    "(+62) 2# #### ####",
    "(+62) 2## #### ####",
    "(+62) 3## #### ####",
    "(+62) 4## #### ####",
    "(+62) 5## #### ####",
    "(+62) 6## #### ####",
    "(+62) 7## #### ####",
    "(+62) 8## #### ####",
    "(+62) 9## #### ####",
];
const DISPLAY_IS_IS: &[&str] = &[
    "+354 ### ####",
    "+354 #######",
    "+354#######",
    "### ####",
    "#######",
];
const DISPLAY_IT_IT: &[&str] = &[
    "+## ### ## ## ####",
    "+## ## #######",
    "+## ## ########",
    "+## ### #######",
    "+## ### ########",
    "+## #### #######",
    "+## #### ########",
    "0## ### ####",
    "+39 0## ### ###",
    "3## ### ###",
    "+39 3## ### ###",
];
const DISPLAY_JA_JP: &[&str] = &[
    "080-####-####",
    "090-####-####",
    "0#-####-####",
    "0####-#-####",
    "0###-##-####",
    "0##-###-####",
    "0##0-###-###",
];
const DISPLAY_KA_GE: &[&str] = &[
    "+995 ### ## ## ##",
    "### ## ## ##",
    "#########",
    "(###) ## ## ##",
    "+995(##)#######",
];
const DISPLAY_KK_KZ: &[&str] = &[
    "+7 (701) #######",
    "+7 (702) #######",
    "+7 (705) #######",
    "+7 (707) #######",
    "+7 (727) 239####",
    "+7 (747) #######",
    "+7 (7172) 745###",
];
const DISPLAY_KO_KR: &[&str] = &[
    "070-####-####",
    "02-####-####",
    "03#-####-####",
    "04#-####-####",
    "05#-####-####",
    "06#-####-####",
    "010-####-####",
    "15##-####",
    "16##-####",
    "18##-####",
];
const DISPLAY_LT_LT: &[&str] = &[
    "86#######",
    "8 6## #####",
    "+370 6## ## ###",
    "+3706#######",
    "(8 5) ### ####",
    "+370 5 ### ####",
    "+370 46 ## ## ##",
    "(8 46) ## ## ##",
];
const DISPLAY_LV_LV: &[&str] = &["########", "## ### ###", "+371 ########"];
const DISPLAY_ME_ME: &[&str] = &[
    "+38220#####",
    "+38267#####",
    "+38269#####",
    "+382679#####",
    "+38268#####",
    "+38240#####",
];
const DISPLAY_MN_MN: &[&str] = &["9#######", "8#######", "7#######", "3#####"];
const DISPLAY_MS_MY: &[&str] = &["{{mobileNumber}}", "{{fixedLineNumber}}", "{{voipNumber}}"];
const DISPLAY_NB_NO: &[&str] = &[
    "+47#########",
    "+47 ## ## ## ##",
    "## ## ## ##",
    "## ## ## ##",
    "########",
    "########",
    "9## ## ###",
    "4## ## ###",
    "9#######",
    "4#######",
];
const DISPLAY_NE_NP: &[&str] = &[
    "01-4######",
    "01-5######",
    "01-6######",
    "9841######",
    "9849######",
    "98510#####",
    "9803######",
    "9808######",
    "9813######",
    "9818######",
];
const DISPLAY_NL_NL: &[&str] = &[
    "06 ########",
    "06-########",
    "+316-########",
    "+31(0)6-########",
    "+316 ########",
    "+31(0)6 ########",
    "01# #######",
    "(01#) #######",
    "+311# #######",
    "02# #######",
    "(02#) #######",
    "+312# #######",
    "03# #######",
    "(03#) #######",
    "+313# #######",
    "04# #######",
    "(04#) #######",
    "+314# #######",
    "05# #######",
    "(05#) #######",
    "+315# #######",
    "07# #######",
    "(07#) #######",
    "+317# #######",
    "0800 ######",
    "+31800 ######",
    "088 #######",
    "+3188 #######",
    "0900 ######",
    "+31900 ######",
];
const DISPLAY_PL_PL: &[&str] = &[
    "+48 ## ### ## ##",
    "0048 ## ### ## ##",
    "### ### ###",
    "+48 ### ### ###",
    "0048 ### ### ###",
    "#########",
    "(##) ### ## ##",
    "+48(##)#######",
    "0048(##)#######",
];
const DISPLAY_PT_PT: &[&str] = &[
    "+351 91#######",
    "+351 92#######",
    "+351 93#######",
    "+351 96#######",
    "+351 21#######",
    "+351 22#######",
    "+351 23#######",
    "+351 24#######",
    "+351 25#######",
    "+351 26#######",
    "+351 27#######",
    "+351 28#######",
    "+351 29#######",
    "91#######",
    "92#######",
    "93#######",
    "96#######",
    "21#######",
    "22#######",
    "23#######",
    "24#######",
    "25#######",
    "26#######",
    "27#######",
    "28#######",
    "29#######",
];
const DISPLAY_RO_MD: &[&str] = &[
    "a## # ## ##",
    "(022) ### ###",
    "+373 60 ### ###",
    "+373 65 0## ###",
    "+373 67 ### ###",
    "+373 68 ### ###",
    "+373 69 ### ###",
    "+373 78 ### ###",
    "+373 79 ### ###",
    "+373 77 4## ###",
    "+373 77 7## ###",
    "+373 77 8## ###",
    "+373 77 9## ###",
    "(373) 60 ### ###",
    "(373) 65 0## ###",
    "(373) 67 ### ###",
    "(373) 68 ### ###",
    "(373) 69 ### ###",
    "(373) 78 ### ###",
    "(373) 79 ### ###",
    "(373) 77 4## ###",
    "(373) 77 7## ###",
    "(373) 77 8## ###",
    "(373) 77 9## ###",
];
const DISPLAY_RU_RU: &[&str] = &[
    "(812) ###-##-##",
    "(495) ###-####",
    "+7 (922) ###-####",
    "(35222) ##-####",
    "8-800-###-####",
];
const DISPLAY_SK_SK: &[&str] = &[
    "+421 ### ### ###",
    "00421 ### ### ###",
    "#### ### ###",
    "00421#########",
    "+421#########",
    "########",
];
const DISPLAY_SL_SI: &[&str] = &[
    "+386 ## ### ###",
    "00386 ## ### ###",
    "0## ### ###",
    "00386########",
    "+386########",
    "0########",
    "+386 # ### ####",
    "00386 # ### ####",
    "0# ### ####",
];
const DISPLAY_SV_SE: &[&str] = &[
    "08-### ### ##",
    "0%#-### ## ##",
    "0%########",
    "+46 (0)%## ### ###",
    "+46(0)%########",
    "+46 %## ### ###",
    "+46%########",
    "08-### ## ##",
    "0%#-## ## ##",
    "0%##-### ##",
    "0%#######",
    "+46 (0)8 ### ## ##",
    "+46 (0)%# ## ## ##",
    "+46 (0)%## ### ##",
    "+46 (0)%#######",
    "+46(0)%#######",
    "+46%#######",
    "08-## ## ##",
    "0%#-### ###",
    "0%#######",
    "+46 (0)%######",
    "+46(0)%######",
    "+46%######",
];
const DISPLAY_TH_TH: &[&str] = &["0 #### ####", "+66 #### ####", "0########"];
const DISPLAY_TR_TR: &[&str] = &[
    "0212#######",
    "0216#######",
    "0222#######",
    "0224#######",
    "050########",
    "053########",
    "054########",
    "055########",
    "0 212 ### ## ##",
    "0 216 ### ## ##",
    "0 222 ### ## ##",
    "0 224 ### ## ##",
    "0 50# ### ## ##",
    "0 53# ### ## ##",
    "0 54# ### ## ##",
    "0 55# ### ## ##",
    "0 (212) ### ## ##",
    "0 (216) ### ## ##",
    "0 (222) ### ## ##",
    "0 (224) ### ## ##",
    "0 (50#) ### ## ##",
    "0 (53#) ### ## ##",
    "0 (54#) ### ## ##",
    "0 (55#) ### ## ##",
    "+90212#######",
    "+90216#######",
    "+90222#######",
    "+90224#######",
    "+9050########",
    "+9053########",
    "+9054########",
    "+9055########",
    "+90 212 ### ## ##",
    "+90 216 ### ## ##",
    "+90 222 ### ## ##",
    "+90 224 ### ## ##",
    "+90 50# ### ## ##",
    "+90 53# ### ## ##",
    "+90 54# ### ## ##",
    "+90 55# ### ## ##",
    "+90 (212) ### ## ##",
    "+90 (216) ### ## ##",
    "+90 (222) ### ## ##",
    "+90 (224) ### ## ##",
    "+90 (50#) ### ## ##",
    "+90 (53#) ### ## ##",
    "+90 (54#) ### ## ##",
    "+90 (55#) ### ## ##",
];
const DISPLAY_UK_UA: &[&str] = &[
    "+38050#######",
    "+38066#######",
    "+38068#######",
    "+38096#######",
    "+38067#######",
    "+38091#######",
    "+38092#######",
    "+38093#######",
    "+38094#######",
    "+38095#######",
    "+38096#######",
    "+38097#######",
    "+38098#######",
    "+38063#######",
    "+38099#######",
    "050#######",
    "066#######",
    "068#######",
    "096#######",
    "067#######",
    "091#######",
    "092#######",
    "093#######",
    "094#######",
    "095#######",
    "096#######",
    "097#######",
    "098#######",
    "063#######",
    "099#######",
    "+38(0##)#######",
    "+38(0###)######",
    "+38(0####)#####",
];
const AU_AREA: &[&str] = &["2", "3", "7", "8"];
const NZ_AREA: &[&str] = &["02", "03", "04", "06", "07", "09"];
const NZ_BEGINNING: &[&str] = &["2", "3", "4", "5", "6", "7", "8", "9"];
const AM_CODE: &[&str] = &[
    "91", "96", "99", "43", "77", "93", "94", "98", "97", "77", "55", "95", "41", "49",
];
const AM_FORMAT: &[&str] = &["######", "##-##-##", "###-###"];
const FI_LAND_CODE: &[&str] = &[
    "02", "03", "05", "06", "08", "09", "013", "014", "015", "016", "017", "018", "019",
];
const FI_MOBILE_CODE: &[&str] = &["040", "050", "044", "045"];
const FI_FORMAT: &[&str] = &["### ####", "#######"];
const VI_AREA: &[&str] = &[
    "76", "281", "64", "781", "240", "241", "75", "650", "56", "651", "62", "780", "26", "710",
    "511", "500", "510", "230", "61", "67", "59", "219", "351", "4", "39", "320", "31", "711",
    "218", "321", "8", "58", "77", "60", "231", "25", "20", "63", "72", "350", "38", "30", "68",
    "210", "57", "52", "510", "55", "33", "53", "79", "22", "66", "36", "280", "37", "54", "73",
    "74", "27", "70", "211", "29", "96", "97", "98", "162", "163", "164", "165", "166", "167",
    "168", "169", "91", "94", "123", "124", "125", "127", "129", "90", "93", "120", "121", "122",
    "126", "128", "92", "186", "188", "99", "199", "95",
];
const DISPLAY_VI_7: &[&str] = &[
    "0[a] ### ####",
    "(0[a]) ### ####",
    "0[a]-###-####",
    "(0[a])###-####",
    "84-[a]-###-####",
    "(84)([a])###-####",
    "+84-[a]-###-####",
];
const DISPLAY_VI_8: &[&str] = &[
    "0[a] #### ####",
    "(0[a]) #### ####",
    "0[a]-####-####",
    "(0[a])####-####",
    "84-[a]-####-####",
    "(84)([a])####-####",
    "+84-[a]-####-####",
];
const DISPLAY_RO_LANDLINE: &[&str] = &[
    "021#######",
    "023#######",
    "024#######",
    "025#######",
    "026#######",
    "027#######",
    "031#######",
    "033#######",
    "034#######",
    "035#######",
    "036#######",
    "037#######",
];
const DISPLAY_RO_MOBILE: &[&str] = &["07########"];

fn display_formats(locale: &str) -> &'static [&'static str] {
    match locale {
        "bg_BG" => DISPLAY_BG_BG,
        "cs_CZ" => DISPLAY_CS_CZ,
        "da_DK" => DISPLAY_DA_DK,
        "de_AT" => DISPLAY_DE_AT,
        "de_CH" | "fr_CH" | "it_CH" => DISPLAY_DE_CH,
        "de_DE" => DISPLAY_DE_DE,
        "el_CY" => DISPLAY_EL_CY,
        "el_GR" => DISPLAY_EL_GR,
        "en_AU" => DISPLAY_EN_AU,
        "en_CA" => DISPLAY_EN_CA,
        "en_GB" => DISPLAY_EN_GB,
        "en_HK" => DISPLAY_EN_HK,
        "en_IN" => DISPLAY_EN_IN,
        "en_NG" => DISPLAY_EN_NG,
        "en_NZ" => DISPLAY_EN_NZ,
        "en_PH" => DISPLAY_EN_PH,
        "en_SG" => DISPLAY_EN_SG,
        "en_UG" => DISPLAY_EN_UG,
        "en_US" => DISPLAY_EN_US,
        "en_ZA" => DISPLAY_EN_ZA,
        "es_AR" => DISPLAY_ES_AR,
        "es_ES" => DISPLAY_ES_ES,
        "es_PE" => DISPLAY_ES_PE,
        "es_VE" => DISPLAY_ES_VE,
        "fa_IR" => DISPLAY_FA_IR,
        "fi_FI" => DISPLAY_FI_FI,
        "fr_BE" | "nl_BE" => DISPLAY_FR_BE,
        "fr_FR" => DISPLAY_FR_FR,
        "he_IL" => DISPLAY_HE_IL,
        "hr_HR" => DISPLAY_HR_HR,
        "hu_HU" => DISPLAY_HU_HU,
        "hy_AM" => DISPLAY_HY_AM,
        "id_ID" => DISPLAY_ID_ID,
        "is_IS" => DISPLAY_IS_IS,
        "it_IT" => DISPLAY_IT_IT,
        "ja_JP" => DISPLAY_JA_JP,
        "ka_GE" => DISPLAY_KA_GE,
        "kk_KZ" => DISPLAY_KK_KZ,
        "ko_KR" => DISPLAY_KO_KR,
        "lt_LT" => DISPLAY_LT_LT,
        "lv_LV" => DISPLAY_LV_LV,
        "me_ME" => DISPLAY_ME_ME,
        "mn_MN" => DISPLAY_MN_MN,
        "ms_MY" => DISPLAY_MS_MY,
        "nb_NO" => DISPLAY_NB_NO,
        "ne_NP" => DISPLAY_NE_NP,
        "nl_NL" => DISPLAY_NL_NL,
        "pl_PL" => DISPLAY_PL_PL,
        "pt_PT" => DISPLAY_PT_PT,
        "ro_MD" => DISPLAY_RO_MD,
        "ru_RU" => DISPLAY_RU_RU,
        "sk_SK" => DISPLAY_SK_SK,
        "sl_SI" => DISPLAY_SL_SI,
        "sv_SE" => DISPLAY_SV_SE,
        "th_TH" => DISPLAY_TH_TH,
        "tr_TR" => DISPLAY_TR_TR,
        "uk_UA" => DISPLAY_UK_UA,
        "zh_TW" => TAIWAN,
        _ => DISPLAY_EN_US,
    }
}
