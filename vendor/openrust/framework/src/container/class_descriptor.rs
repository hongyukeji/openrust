//! 编译类目录与构造参数。Resolver 负责依赖选择、求值、所有权和发布。
pub use self::php_scalar::{PhpBytes, PhpIntBool, PhpStringable, PhpTarget, ScalarDiagnostic};
use super::class_identity::{ClassIdentities, ClassIdentity};
use super::class_projection::ClassProjection;
use super::parameters::ParameterValue;
#[cfg(test)]
use super::parameters::{ParameterKey, ParameterOverrides};
use super::{ServiceKey, Value, container_error};
use crate::{Error, Result};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type CheckValue = fn(&ParameterValue) -> Result<()>;
type DefaultValue = fn() -> Result<ParameterValue>;
type Construct = dyn for<'a> Fn(ResolvedArgs<'a>) -> Result<Value> + Send + Sync;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ClassKind {
    Concrete,
    Interface,
    Abstract,
}

/// 依赖选择保留类键，普通标量保留明确 Rust 类型。
pub(super) enum ParameterKind {
    Scalar {
        #[cfg(test)]
        rust_type: TypeId,
        check: CheckValue,
    },
    Php(PhpTarget),
    #[cfg(test)]
    Untyped,
    Class {
        key: ServiceKey,
        erased_output: TypeId,
        check: CheckValue,
    },
}

/// 一个构造参数的编译期声明。参数顺序与构造回调读取的槽位一致。
pub struct ConstructorParam {
    name: &'static str,
    kind: ParameterKind,
    nullable: bool,
    variadic: bool,
    default: Option<DefaultValue>,
}

impl ConstructorParam {
    fn new(name: &'static str, kind: ParameterKind) -> Self {
        Self {
            name,
            kind,
            nullable: false,
            variadic: false,
            default: None,
        }
    }

    /// 声明严格读取为指定 Rust 类型的普通值。
    pub fn scalar<T: Send + Sync + 'static>(name: &'static str) -> Self {
        Self::new(
            name,
            ParameterKind::Scalar {
                #[cfg(test)]
                rust_type: TypeId::of::<T>(),
                check: check::<T>,
            },
        )
    }

    /// 声明按指定 PHP 标量规则物化的构造参数。
    ///
    /// 整批参数完成选择后才转换。读取时使用 [`PhpTarget`] 对应的输出类型。
    #[must_use]
    pub fn php(name: &'static str, target: PhpTarget) -> Self {
        Self::new(name, ParameterKind::Php(target))
    }

    #[cfg(test)]
    pub(super) fn untyped(name: &'static str) -> Self {
        Self::new(name, ParameterKind::Untyped)
    }

    /// 声明由同一 Resolver 解析的类型化类依赖。
    pub fn class<T: ?Sized + Send + Sync + 'static>(name: &'static str) -> Self {
        Self::new(
            name,
            ParameterKind::Class {
                key: ServiceKey::typed::<T>(),
                erased_output: TypeId::of::<Arc<T>>(),
                check: check::<Arc<T>>,
            },
        )
    }

    /// 接受显式空值。未提供默认值的类参数仍会尝试解析依赖。
    #[must_use]
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// 声明构造器末尾接收零项或多项的同类型实参。
    #[must_use]
    pub fn variadic(mut self) -> Self {
        self.variadic = true;
        self
    }

    /// 无对应绑定或普通值来源时调用的默认值函数。
    #[must_use]
    pub fn with_default(mut self, default: fn() -> Result<ParameterValue>) -> Self {
        self.default = Some(default);
        self
    }

    pub(super) fn name(&self) -> &'static str {
        self.name
    }

    pub(super) fn kind(&self) -> &ParameterKind {
        &self.kind
    }

    /// 命中后总是一个实参。显式 null 与 Index 均按其实际身份处理。
    #[cfg(test)]
    pub(super) fn override_argument(&self, overrides: &ParameterOverrides) -> Option<Argument> {
        overrides
            .get(&ParameterKey::name(self.name))
            .cloned()
            .map(Argument::One)
    }

    pub(super) fn has_default(&self) -> bool {
        self.default.is_some()
    }

    pub(super) fn is_variadic(&self) -> bool {
        self.variadic
    }

    pub(super) fn allows_null(&self) -> bool {
        self.nullable
    }

    /// 仅在调用方已选中默认分支时求值。原业务错误直接传回。
    pub(super) fn evaluate_default(&self) -> Result<Option<ParameterValue>> {
        let Some(default) = self.default else {
            return Ok(None);
        };
        let value = default()?;
        if let Err(primary) = self.validate_selected(&value) {
            let report = crate::foundation::lifecycle::disposal::dispose(value, None);
            return Err(if report.seen {
                crate::foundation::lifecycle::TaskExecutionFailure::error(
                    primary,
                    container_error("resource_task_interrupted", "默认值退役包含异常"),
                )
            } else {
                primary
            });
        }
        Ok(Some(value))
    }

    /// 严格参数保持选参时校验，弱标量在整批参数选定后物化。
    pub(super) fn validate_selected(&self, value: &ParameterValue) -> Result<()> {
        if matches!(&self.kind, ParameterKind::Php(_)) {
            Ok(())
        } else {
            self.validate_value(value)
        }
    }

    fn validate_value(&self, value: &ParameterValue) -> Result<()> {
        match &self.kind {
            ParameterKind::Php(_) => Err(container_error(
                "container_constructor_weak_frame_required",
                "弱标量声明需要已选参数帧和诊断处理入口",
            )),
            _ if value.is_null() => self.validate_null(),
            ParameterKind::Scalar { check, .. } | ParameterKind::Class { check, .. } => {
                check(value)
            }
            #[cfg(test)]
            ParameterKind::Untyped => Ok(()),
        }
    }

    fn validate_null(&self) -> Result<()> {
        #[cfg(test)]
        if matches!(self.kind, ParameterKind::Untyped) {
            return Ok(());
        }
        if self.nullable {
            Ok(())
        } else {
            Err(container_error(
                "container_parameter_null",
                "构造参数不接受显式 null",
            ))
        }
    }

    fn materialize_value(
        &self,
        value: &ParameterValue,
        diagnostic: &mut impl FnMut(ScalarDiagnostic) -> Result<()>,
    ) -> Result<ParameterValue> {
        match &self.kind {
            ParameterKind::Php(target) if !value.is_null() => target.materialize(value, diagnostic),
            ParameterKind::Php(_) => self.validate_null().map(|()| value.clone()),
            _ => self.validate_value(value).map(|()| value.clone()),
        }
    }
}

fn check<T: Send + Sync + 'static>(value: &ParameterValue) -> Result<()> {
    value.downcast::<T>().map(drop)
}

/// 声明槽位的有序解析结果。一个 Vec 值可独立作为 One，Many 才代表参数列。
pub(super) enum Argument {
    One(ParameterValue),
    Many(Vec<ParameterValue>),
}

/// 借用当前已校验的构造参数帧。构造回调通过从零开始的槽位读取依赖。
pub struct ResolvedArgs<'a> {
    parameters: &'a [ConstructorParam],
    arguments: &'a [Argument],
}
impl<'a> ResolvedArgs<'a> {
    fn validate(parameters: &'a [ConstructorParam], arguments: &'a [Argument]) -> Result<Self> {
        validate_argument_count(parameters, arguments)?;
        for (parameter, argument) in parameters.iter().zip(arguments) {
            match argument {
                Argument::One(value) => parameter.validate_value(value)?,
                Argument::Many(values) if parameter.variadic => {
                    for value in values {
                        parameter.validate_value(value)?;
                    }
                }
                Argument::Many(_) => return Err(argument_shape()),
            }
        }
        Ok(Self {
            parameters,
            arguments,
        })
    }

    fn materialize_selected(
        parameters: &[ConstructorParam],
        selected: &[Argument],
        diagnostic: &mut impl FnMut(ScalarDiagnostic) -> Result<()>,
        check: &mut impl FnMut() -> Result<()>,
    ) -> Result<Vec<Argument>> {
        validate_argument_count(parameters, selected)?;
        if parameters
            .iter()
            .zip(selected)
            .any(|(parameter, argument)| {
                !parameter.variadic && matches!(argument, Argument::Many(_))
            })
        {
            return Err(argument_shape());
        }
        let mut materialize = |parameter: &ConstructorParam, value: &ParameterValue| {
            check()?;
            parameter.materialize_value(value, diagnostic)
        };
        parameters
            .iter()
            .zip(selected)
            .map(|(parameter, argument)| match argument {
                Argument::One(value) => materialize(parameter, value).map(Argument::One),
                Argument::Many(values) => values
                    .iter()
                    .map(|value| materialize(parameter, value))
                    .collect::<Result<Vec<_>>>()
                    .map(Argument::Many),
            })
            .collect()
    }

    fn slot(&self, index: usize) -> Result<(&ConstructorParam, &Argument)> {
        self.parameters
            .get(index)
            .zip(self.arguments.get(index))
            .ok_or_else(|| {
                container_error("container_constructor_argument_slot", "构造参数槽不存在")
            })
    }

    /// 借用指定槽位的普通值，可检查是否为空或按声明类型读取。
    pub fn value(&self, index: usize) -> Result<&ParameterValue> {
        match self.slot(index)? {
            (parameter, Argument::One(value)) if !parameter.variadic => Ok(value),
            _ => Err(argument_shape()),
        }
    }

    /// 借用变参槽的有序实参。显式单值覆盖视为一项。
    pub fn values(&self, index: usize) -> Result<&[ParameterValue]> {
        let (parameter, argument) = self.slot(index)?;
        if !parameter.variadic {
            return Err(argument_shape());
        }
        Ok(match argument {
            Argument::One(value) => std::slice::from_ref(value),
            Argument::Many(values) => values,
        })
    }

    /// 克隆指定槽位的类依赖。槽位、声明类型或值不匹配时返回错误。
    pub fn service<T: ?Sized + Send + Sync + 'static>(&self, index: usize) -> Result<Arc<T>> {
        let (parameter, _) = self.slot(index)?;
        if !matches!(&parameter.kind, ParameterKind::Class { key, .. } if key == &ServiceKey::typed::<T>())
        {
            return Err(container_error(
                "container_parameter_type_mismatch",
                "构造参数的类声明与请求类型不同",
            ));
        }
        self.value(index)?
            .downcast::<Arc<T>>()
            .map(|value| Arc::clone(value.as_ref()))
    }
}

fn validate_argument_count(parameters: &[ConstructorParam], arguments: &[Argument]) -> Result<()> {
    if parameters.len() == arguments.len() {
        Ok(())
    } else {
        Err(container_error(
            "container_constructor_argument_count",
            "构造参数槽数与声明不一致",
        ))
    }
}

fn argument_shape() -> Error {
    container_error(
        "container_constructor_argument_shape",
        "构造参数槽的单值或变参形状不匹配",
    )
}

struct Constructor {
    parameters: Vec<ConstructorParam>,
    output: TypeId,
    construct: Option<Box<Construct>>,
}

impl Drop for Constructor {
    fn drop(&mut self) {
        // 无效整批退役时各构造捕获独立释放，沿既有同步责任报告本次诊断。
        crate::foundation::lifecycle::disposal::dispose(self.construct.take(), None);
    }
}

/// 可加入应用启动类目录的编译期描述。
///
/// 描述符提供普通构造入口。显式绑定优先，描述符本身不登记服务绑定。
pub struct ClassDescriptor {
    identity: ClassIdentity,
    key: ServiceKey,
    kind: ClassKind,
    constructor: Option<Constructor>,
    projections: Vec<ClassProjection>,
}
impl ClassDescriptor {
    fn declare<T: ?Sized + 'static>(
        symbol: &'static str,
        kind: ClassKind,
        constructor: Option<Constructor>,
    ) -> Self {
        Self {
            identity: ClassIdentity::of::<T>(symbol),
            key: ServiceKey::typed::<T>(),
            kind,
            constructor,
            projections: Vec::new(),
        }
    }

    /// 描述具体类型及其构造依赖。符号与类型在同一目录中各自唯一。
    ///
    /// 无显式绑定时，普通解析使用回调构造实例，依赖先在当前作用域内解析。
    pub fn concrete<T: Send + Sync + 'static>(
        symbol: &'static str,
        parameters: Vec<ConstructorParam>,
        new: impl for<'a> Fn(ResolvedArgs<'a>) -> Result<Arc<T>> + Send + Sync + 'static,
    ) -> Self {
        Self::declare::<T>(
            symbol,
            ClassKind::Concrete,
            Some(Constructor {
                parameters,
                output: TypeId::of::<Arc<T>>(),
                construct: Some(Box::new(move |args| new(args).map(super::erase))),
            }),
        )
    }

    /// 声明接口的符号与类型。解析需要显式绑定或别名。
    pub fn interface<T: ?Sized + Send + Sync + 'static>(symbol: &'static str) -> Self {
        Self::declare::<T>(symbol, ClassKind::Interface, None)
    }

    /// 声明不具备普通构造入口的抽象类型。
    pub fn abstract_class<T: ?Sized + Send + Sync + 'static>(symbol: &'static str) -> Self {
        Self::declare::<T>(symbol, ClassKind::Abstract, None)
    }

    /// 增加由 [`crate::class_projection!`] 声明的同对象视图。
    #[must_use]
    pub fn with_projection(mut self, projection: ClassProjection) -> Self {
        self.projections.push(projection);
        self
    }
}

pub(super) struct ClassRecord {
    key: ServiceKey,
    kind: ClassKind,
    constructor: Option<Constructor>,
    projections: HashMap<ServiceKey, ClassProjection>,
}
impl ClassRecord {
    pub(super) fn kind(&self) -> ClassKind {
        self.kind
    }

    fn prepare(&self, output: &ServiceKey) -> Result<PreparedConstructor<'_>> {
        let constructor = self.constructor.as_ref().ok_or_else(|| {
            super::unresolvable_dependency(
                "container_class_not_instantiable",
                "抽象类或接口不能直接构造",
            )
        })?;
        let projection = if &self.key == output {
            None
        } else {
            Some(self.projections.get(output).ok_or_else(|| {
                container_error("service_type_mismatch", "编译类未声明请求的输出类型")
            })?)
        };
        Ok(PreparedConstructor {
            constructor,
            projection,
        })
    }
}

/// 装配前一次冻结。两个索引只描述同一批已编译类。
#[derive(Default)]
pub(super) struct ClassRegistry {
    identities: ClassIdentities,
    by_type: HashMap<TypeId, ClassRecord>,
    projection_sources: HashMap<TypeId, TypeId>,
}
impl ClassRegistry {
    pub(super) fn from_descriptors(descriptors: Vec<ClassDescriptor>) -> Result<Self> {
        let mut identities = Vec::with_capacity(descriptors.len());
        let mut drafts = Vec::with_capacity(descriptors.len());
        for ClassDescriptor {
            identity,
            key,
            kind,
            constructor,
            projections,
        } in descriptors
        {
            identities.push(identity);
            drafts.push((key, kind, constructor, projections));
        }
        let identities = ClassIdentities::from_entries(identities)?;
        let mut by_type = HashMap::with_capacity(drafts.len());
        let mut projection_sources = HashMap::new();
        for (key, kind, constructor, projections) in drafts {
            let id = key.type_id().ok_or_else(|| {
                container_error(
                    "container_class_identity_duplicate",
                    "编译类没有规范类型身份",
                )
            })?;
            if let Some(constructor) = &constructor {
                validate_parameters(&constructor.parameters)?;
            }
            let mut by_target = HashMap::with_capacity(projections.len());
            for projection in projections {
                if kind != ClassKind::Concrete
                    || projection.source_key() != &key
                    || projection.target_key() == &key
                    || by_target.contains_key(projection.target_key())
                {
                    return Err(container_error(
                        "container_class_projection_invalid",
                        "编译类投影的源或目标不合法",
                    ));
                }
                by_target.insert(projection.target_key().clone(), projection);
            }
            if !by_target.is_empty()
                && let Some(constructor) = &constructor
            {
                projection_sources.insert(constructor.output, id);
            }
            by_type.insert(
                id,
                ClassRecord {
                    key,
                    kind,
                    constructor,
                    projections: by_target,
                },
            );
        }
        Ok(Self {
            identities,
            by_type,
            projection_sources,
        })
    }

    pub(super) fn projects_type(&self, source: &ServiceKey, target: &ServiceKey) -> bool {
        let target = self.canonical_key(target);
        self.class(source)
            .is_some_and(|class| class.projections.contains_key(&target))
    }

    pub(super) fn canonical_key(&self, key: &ServiceKey) -> ServiceKey {
        self.identities.canonical_key(key)
    }

    pub(super) fn canonical_key_owned(&self, key: ServiceKey) -> ServiceKey {
        self.identities.canonical_key_owned(key)
    }

    #[cfg(test)]
    pub(super) fn identity_only(identities: ClassIdentities) -> Self {
        Self {
            identities,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn identities(&self) -> &ClassIdentities {
        &self.identities
    }

    pub(super) fn has_descriptors(&self) -> bool {
        !self.by_type.is_empty()
    }

    pub(super) fn class(&self, key: &ServiceKey) -> Option<&ClassRecord> {
        let id = self.identities.canonical_key(key).type_id()?;
        self.by_type.get(&id)
    }

    pub(super) fn output_projection(
        &self,
        declared_output: TypeId,
        target: &ServiceKey,
    ) -> Option<&ClassProjection> {
        let id = self.projection_sources.get(&declared_output)?;
        self.by_type.get(id)?.projections.get(target)
    }

    /// 先选中一个具体源，再检查该源的声明输出。目录不反向查找 trait 实现。
    pub(super) fn prepare(
        &self,
        key: &ServiceKey,
        output: &ServiceKey,
    ) -> Result<PreparedConstructor<'_>> {
        let class = self.class(key).ok_or_else(|| {
            super::unresolvable_dependency("container_class_missing", "类未在本次编译目录中声明")
        })?;
        class.prepare(&self.identities.canonical_key(output))
    }
}

fn validate_parameters(parameters: &[ConstructorParam]) -> Result<()> {
    let mut names = HashSet::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        if parameter.name.is_empty() || !names.insert(parameter.name) {
            return Err(container_error(
                "container_constructor_parameter_name",
                "构造参数名称为空或重复",
            ));
        }
        if parameter.variadic && (index + 1 != parameters.len() || parameter.default.is_some()) {
            return Err(container_error(
                "container_constructor_variadic_invalid",
                "变参必须位于最后且不能声明默认值",
            ));
        }
    }
    Ok(())
}

/// 预检结果只借用已冻结的类记录，不拥有依赖值、缓存或 Scope。
pub(super) struct PreparedConstructor<'a> {
    constructor: &'a Constructor,
    projection: Option<&'a ClassProjection>,
}
impl PreparedConstructor<'_> {
    pub(super) fn parameters(&self) -> &[ConstructorParam] {
        &self.constructor.parameters
    }

    pub(super) fn has_weak_parameters(&self) -> bool {
        self.parameters()
            .iter()
            .any(|parameter| matches!(parameter.kind(), ParameterKind::Php(_)))
    }

    pub(super) fn construct(&self, arguments: &[Argument]) -> Result<Value> {
        let resolved = ResolvedArgs::validate(self.parameters(), arguments)?;
        self.invoke_constructor(resolved)
    }

    #[cfg(test)]
    fn construct_selected(
        &self,
        selected: &[Argument],
        diagnostic: &mut impl FnMut(ScalarDiagnostic) -> Result<()>,
    ) -> Result<Value> {
        self.construct_selected_checked(selected, diagnostic, &mut || Ok(()))
    }

    /// 借用整批已选源帧。每次物化或构造前核对来源与资源归属。
    pub(super) fn construct_selected_checked(
        &self,
        selected: &[Argument],
        diagnostic: &mut impl FnMut(ScalarDiagnostic) -> Result<()>,
        check: &mut impl FnMut() -> Result<()>,
    ) -> Result<Value> {
        let parameters = self.parameters();
        let arguments =
            ResolvedArgs::materialize_selected(parameters, selected, diagnostic, check)?;
        check()?;
        self.invoke_constructor(ResolvedArgs {
            parameters,
            arguments: &arguments,
        })
    }

    fn invoke_constructor(&self, resolved: ResolvedArgs<'_>) -> Result<Value> {
        let construct =
            self.constructor.construct.as_ref().ok_or_else(|| {
                container_error("container_class_not_instantiable", "构造回调已退役")
            })?;
        construct(resolved)
    }

    /// 构造输出或既有规范缓存值均可用于此处，视图不创建第二个对象。
    pub(super) fn project(&self, source: &Value) -> Result<Value> {
        if source.as_ref().type_id() != self.constructor.output {
            return Err(container_error(
                "service_type_mismatch",
                "规范值与编译类的声明输出不同",
            ));
        }
        match self.projection {
            Some(projection) => projection.project(source),
            None => Ok(Arc::clone(source)),
        }
    }
}

#[cfg(test)]
type ClassDirectory = ClassRegistry;

#[cfg(test)]
#[path = "class_descriptor_tests.rs"]
mod tests;

#[path = "php_scalar.rs"]
mod php_scalar;

#[cfg(test)]
#[path = "php_scalar_tests.rs"]
mod php_scalar_tests;
